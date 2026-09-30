# P2-B 实施与验证报告

2026-09-28；基线 `main@4514630dcd26481cf6dbc2aff38824ed71ef06da`。**P2-006～010 五项完成；累计50项done、142项todo。P2完成10/20；整个P2/G2尚未完成，下一批P2-C。** 恢复中断留下的P2-B源码与失败记录，继续修复并以当前冻结源码重新验收。未提交、推送或创建PR，没有embedding/模型调用，没有改动全局SDK/默认工具链或开发者日常索引。

## 1. 五项实际交付

| 任务 | 实现与验收范围 |
|---|---|
| P2-006 | 复用已有AST提取Go package、导出/包级名称、类型/别名/方法签名；PackageKey隔离目录、包名和生产/测试贡献；包成员增删改变聚合表面；共享每包名称索引 |
| P2-007 | Java、C/C++、spec-driven/generic保守能力表，保留具体语言与Unknown原因；已有符号/边提取不被冒充为完整接口或编译器能力 |
| P2-008 | ResolutionOutcome四类状态；持久化稳定目标身份、策略/置信度或歧义候选/原因/截断；结果覆盖统计贯穿IndexReport |
| P2-009 | 语义同分保持歧义，候选稳定展示；目录缓存保留seed-token/tombstone与增量维护；类型和别名冲突不再最后插入者获胜 |
| P2-010 | 同文件事务保存正/负查找依赖并建立反向索引；目标、名字桶、路径候选、包贡献和已识别配置变化进入原有有界dirty closure |

落点：`cc-model/{package_surface,resolution}.rs`；`cc-parsers/exports/{go,conservative}.rs`；`cc-db/resolution_dependency_store.rs`；`cc-index/resolver/evidence.rs`、`indexer_phases/dependencies.rs` 与现有解析编排。继续七crate、默认离线、14个MCP工具；没有另造增量引擎或新增低层工具。

## 2. 解析与存储合同

`Resolved / Ambiguous / Unresolved / Unsupported` 分开，内部目录槽位不进入持久证据。相同分数的候选不能仅因排序稳定就把第一项当作唯一目标。超过候选扫描额度时保留truncated与实际可证明的数量下界，不把未枚举桶报告为唯一或完整空结果。重复import别名的目标冲突同样保留歧义。

`resolution_manifests`与`resolution_dependencies`随文件、符号和边同事务提交，删除级联；dirty-only替换结果/依赖但保留未重解析的PublicSurface。坏版本、坏摘要、冲突site、重复候选、无效置信度等显式失败。每文件结果4096条、普通依赖4096条、每项歧义32候选；生成预算4MiB，持久JSON硬上限6MiB。超出时incomplete/omitted并补最多3个全局保守依赖，不静默掉失效事件。规范化和反序列化后继续追加不会重置预算；最终投影不会复活已撤销旧目标或抹掉不完整原因。

`resolution_coverage`是本次处理文件（包含dirty-only）的统计，不是全仓正确率；`public_surface_coverage`仍只统计本次parse文件。核心契约和边界详见 `docs/internals/RESOLUTION_DEPENDENCIES.md` 与 `PUBLIC_SURFACE.md`。

## 3. Go及不支持能力的界线

Go包视图按目录/包名/测试贡献组合。内部测试能读取生产包，生产包不读取测试声明，外部测试包不与生产包混合。包级声明可参与内部依赖，函数体局部变量不污染表面；带receiver的方法不当作裸包函数。500个消费文件的测试验证共享同一Arc视图，不逐文件复制整包，也不逐调用扫描全部包文件。它是结构/共享机制验证，不是500文件的性能认证。

build tags、GOOS/GOARCH、cgo、dot import、动态推断等未被求值，表面Unknown；目前是准入源码的静态贡献，不是Go编译器选件后的结果。Java/C/C++完整classpath、宏/模板及其他语言运行时语义未实现，不能把基础AST/regex提取等同于这些能力。

原Cargo reader只把workspace成员package名映射到入口，不支持`[dependencies]`重命名。初版测试错误假定其已支持；保留失败记录，新增显式不支持行为测试与独立的已支持package-name配置变更测试。前者证明会重解析但不捏造目标，后者证明未修改调用者会重新绑定到新入口。没有改benchmark答案来迎合结果，也没有本轮扩写P3模块解析器。

## 4. 本轮审查额外修复的实际缺口

**Rust链式调用ID碰撞。** 多个链式调用共用receiver起点，导致不同调用具有同一ID，落库可能相互覆盖。改用实际callee token位置；raw parser红测试及实际全量/增量对照验证四个route调用完整保留。未从oracle删除ID或源码位置。

**Express AST被正则重复覆盖。** 原样例3条AST路由被富化成6条，命名函数可能被误识别为function关键字；同一行不同注册又都使用列0。现在AST注册位置优先，正则仅补未占用位置，并使用实际字节列。保留两条有效红测试与修复后断言。

**全量与增量route-node主键不同。** 全量staging曾写NULL edge_id，增量使用route_id，十一表oracle据此失败。单节点插入改为委托现有同一个非空主键批写助手；未放宽route比较。

**P2-B草稿边界。** 检出并修复预算normalize后重置、重复候选伪歧义、Go方法被当包函数、最终seal复活已撤销目标等；这些分类为新实现审查问题，不冒充P2-A产品收益。开发编译/lint/测试前提失败也单独保留，见 `evidence-classification.json`。

## 5. 冻结源码的最终严格验证

最终覆盖SHA-256：`f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d`，477个覆盖文件；38条最终命令退出0。原始argv、退出码、日志、工具链与二进制摘要位于 `artifacts/benchmarks/p2b-20260928/final/validation.json`。配对完成后再次校验源码无漂移。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---|---|
| 全仓all-targets+eval-http严格Clippy，-D warnings | exit0 | exit0 |
| Workspace完整测试，含doctest | 1494 passed / 0 failed / 30 ignored | 1494 passed / 0 failed / 30 ignored |
| cc-eval+eval-http | 135 passed / 0 failed / 24 ignored | 135 passed / 0 failed / 24 ignored |
| Workspace/eval二进制构建 | exit0 | exit0 |
| 新P2-B真实MCP | 1 passed | 1 passed |
| 原P2-A真实MCP | 1 passed | 1 passed |
| P1-D成本/文档/并发MCP | 1 / 1 / 1 passed | 同左 |
| P1-C工具契约/范围解释MCP | 1 / 1 passed | 同左 |
| P1-B / P1-A / P0 MCP | 2 / 2 / 1 passed | 同左 |
| Watcher专项 / 核心并发额外重复 | 10 passed / 2次通过 | 同左 |

不同命令包含重叠测试，不相加；ignored不计成功，真实MCP按显式命令另跑。仅macOS arm64本地验证；SDK15.4只作用于子进程，未修改默认工具链；无Linux/Windows/远端Actions运行证明。CI新增了本批MCP步骤，但配置存在不等于远端执行成功。

## 6. 增量正确性证据

P2-B专项13项非ignored测试通过，包括缺失名补全、全局唯一→歧义→唯一、缺失路径和更优先入口出现、Go包成员增删/签名/测试隔离、缓存历史、事件范围prepare/commit、配置变化、body/no-op、路线ID以及超限状态。归档了21个规范比较检查点（包含沿用Python/Rust回归），均一致；另有独立断言检查正确目标、撤销旧边、歧义候选和包边界，不能让两侧共同丢边而判成功。

当前oracle对照11张指定表，而不是声称覆盖整个数据库：resolution_manifests、resolution_dependencies、public_surfaces、files、symbols、imports、symbol_refs、call_edges、chunks、test_edges、routes。P2-A旧runner保持其9表口径。原Python/Rust签名mutation两版均equal=true/exit0，说明P2-A修复没有退化。

超限样例：3个旧消费者，dirty预算1，实际只刷新1个消费者，report=budget_exceeded；显式全量重建后3个消费者全部刷新。首次计数把新provider的声明调用算进消费者，属于测试统计前提错误，已排除provider并保留原失败。**预算剩余工作的持久化和自动恢复尚未完成，属于P2-C；不能把当前一次正常/no-op构建当成此前剩余工作已收敛。**

## 7. 固定输入检索benchmark

同一个当前cc-eval、同一冻结源码/题库/答案/配置，仅替换P2-A/P2-B产品二进制；8组、306次真实MCP请求。逐题Top-1/nDCG delta均0，无新增范围/源码无效证据；8组metrics、query-slices与costs均完成离线回放，摘要一致。14工具新旧输入schema/未知字段合同收据一致。

| 数据 | 每版独立题/请求 | P2-A Top1 / nDCG@10 | P2-B Top1 / nDCG@10 |
|---|---:|---:|---:|
| 原七文件源码集 | 14 / 42 | 0.8571428571 / 0.9379235538 | 0.8571428571 / 0.9379235538 |
| 原三文件 smoke | 11 / 33 | 0.7000000000 / 0.7000000000 | 0.7000000000 / 0.7000000000 |
| 定位/范围回归 | 8 / 24 | 1.0000000000 / 1.0000000000 | 1.0000000000 / 1.0000000000 |
| 六文件双语意图 | 18 / 54 | 0.6250000000 / 0.6562500000 | 0.6250000000 / 0.6562500000 |

smoke质量均值只包含10道正答案题；S11每版仍3次no-answer失败、原始gate保持exit1。正式三组compare均为inconclusive，不能由小样本p95比值宣称加速。四道无标识符中文题的既有语义召回限制未解决。本批改善的是结构/增量正确性，不是新增语义召回收益。

当前源码CI题库只刷新source摘要；问题和答案hash不变，15个历史冻结输入文件hash全部不变。`dataset-migration.json`将这次更新与配对输入分开，没有覆盖旧数据或旧结果。

## 8. 交接、回滚与未完成项

数据库schema **12**；P2-A的10及未验收草稿11不能复用为兼容缓存，升级/回滚需隔离缓存全量重建。本轮未重建开发者日常索引；原有持久资产导出恢复界线保留。55个被测源/测试/文档文件相对P2-A的新增或修改在 `p2b-change-map.json` 和 `p2b-from-p2a.patch` 可复核。

权威任务状态只更新tasks.json，再由scripts/code_index_plan.py派生05-TODO。同步修正历史遗留的根current_phase/next_task提示，避免它们仍指向P0。P2-B-GATE为passed_local_batch，不是G2/M1发布通过。下一批P2-C（P2-011～015）：完整变化分类/DirtyPlanner、重导出环固定点、所有重载目标字段审计、持久frontier与freshness传播到查询/MCP。已完成基础依赖与清理不重复实现。

未运行embedding、真实OCE服务、LSP/编译器、付费provider、holdout、多仓600题、100k、release尾延迟或峰值内存认证。不把Unknown/partial重新命名为全量成功。证据包包含继承源码、P2-B原始失败及最终验证，不含.git、构建目录或产品二进制，不是发布包。
