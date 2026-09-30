# P3-C｜Go项目模型、模块结果与配置事件对账

状态：P3-C五项任务在声明的本地静态范围验收完成。累计75 done、117 todo；P3完成15/20，下一批P3-D。G3未完成，非完整编译器/运行时或发行认证。

基线 main@4514630dcd26481cf6dbc2aff38824ed71ef06da。入口逐文件核对P3-B final-v3覆盖集合，无漂移；已有dirty工作保留。入口快照为 artifacts/benchmarks/p3c-20260928-4eCI/entry.json 与 entry-source.tar.gz。无commit/push/PR/merge或日常索引清理。

## 实际实现与任务映射

| 任务 | 实际落点与范围 |
|---|---|
| P3-011 | cc-model/go_project.rs、project_model/go.rs/go_capture.rs、parsers/go_modules.rs、module_resolution/go.rs：捕获go.mod/go.work、本地replace、声明包名、多文件包集合及显式build条件。无需安装依赖或执行源码。 |
| P3-012 | cc-model/project_model.rs/resolution.rs、各语言module_resolution及resolver：内部文件/包集合/External/Ambiguous/Unknown分别表达，不拿None混合missing与external。包目标不伪造代表文件，实际调用/引用指向具体符号。 |
| P3-013 | project_model/mod.rs复用既有dirty/dependencies/frontier；Go配置-only可分批重定向并跨MCP进程重启，与全量十四表结果一致。已知无条件本地Go结果记录具体正负配置/成员依赖，未知结果仍保守失效；TS/Cargo原回归保留。 |
| P3-014 | watcher.rs和project_session.rs：分辨重命名端点、目录/溢出/错误转全树对账，消费端实际使用rescan而不只记录日志；发布失败重排rescan请求。不是新建watcher或恢复引擎。 |
| P3-015 | cc-index/tests/p3c_modules.rs、cc-eval/tests/p3c_modules.rs、benchmarks/modules/p3c-fixtures.json和scripts/module_oracles.py；独立手写gold与实际parser/SQLite/MCP分层验证，可用Rust/Python外部工具交叉验证，缺少Go/tsc不宣称通过。 |

原计划路径含尚未存在的incremental/dependencies.rs、module_resolution_matrix.rs等，本批使用已存在的职责边界与本批具名测试，没有为满足设计树重复建引擎。最终任务scope按实际落点更新。

## 修复的关键缺口

初始red-go-module的两个测试失败证明本地Go replace包成员未解析、外部依赖没有独立模块状态。模块结果现在具有package集合，SQL imports.resolved_path为NULL不再等同于解析失败；模块manifest/coverage解释真实状态。

Go selector原parser会用叶名称绑定同文件函数；现在选择器不获得这种parser_exact，调用和引用均携带包接收者的解析依据。声明包名与导入路径末段不一致仍可解析。实际导出的Go函数也不因首字母大写被归为constructor。负例保留同名本地/外部干扰源。

watcher原先将rename两侧都当changed、忽略错误事件；更重要的是虽然记录rescan_needed，消费端仍一律构造局部scope。修复后原生事件分类保留不完整信号，consumer传None触发全树扫描。受保护/缓存/构建目录仍排除，避免索引自己的写入不断触发扫描。失败构建不再消耗掉唯一的重试信号。

原P3-B重复包名测试的Unsupported预期迁移为Ambiguous，并增加两个候选的断言，原歧义原因及无伪目标断言保留。开发过程编译和Clippy反馈均留日志，不改写为最终成功。

## 验证与证据边界

最终接受 artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json：548个源码/测试/运行文档/CI/脚本文件，摘要2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961；27条Cargo命令全部exit0。scope-check.json复核集合、内容、日志、二进制、原始配对、外部工具输入及Git状态通过。路线图状态单独检查，不混入循环哈希。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 全仓严格Clippy/全部目标/HTTP | 通过 | 通过 |
| workspace含doctest | 1611 passed / 41 ignored | 1611 passed / 41 ignored |
| HTTP | 175 passed / 35 ignored | 175 passed / 35 ignored |
| 显式真实MCP | 17 passed | 17 passed |
| watcher专项 | 17 passed | 17 passed |

命令覆盖重叠，不相加；ignored不算通过。仅macOS arm64，本轮未运行远端Actions或跨平台CI。

unit层：包集合、声明包名、workspace/replace优先级、外部/缺失/未知/歧义、忽略配置、cgo/build条件、离线快照、跨语言结果状态及结构约束。

integration层：实际parser/SQLite、同名干扰的精确目标和引用、配置-only增量/重启/全量一致。controlled watcher层：注入结构/错误/overflow事件，以及实际watcher tick仅凭rescan标志发现未列出的配置变化；损坏输入缓存造成构建失败后仍重试。原生本地watcher旧测试保留，但不冒充所有OS的overflow测试。

MCP层：独立产品子进程、隔离HOME/cache、public index/graph_query、go.work-only改变、预算一的三个消费者、中途关闭并重开进程；零消费者源码重解析、完整前不得报complete，最终十四表与全量相同。

外部oracle层：同一固定手写fixture被产品测试和独立工具分别消费。Rust --emit metadata/dep-info仅比较已加载的源文件，不运行输出程序或Cargo build scripts；Python PathFinder显式路径查找不执行会抛错的initializer。最终两个工具链运行均通过相同四个Rust/Python案例，不重复计成八个独立案例；Go编译器在当前运行环境未找到，两个Go外部案例not_run，TypeScript编译器参照也not_run。版本、命令、原始输出与输入/harness哈希分别保存于final-oracles-stable和final-oracles-1.95.0；不等于全规则编译器验证。

release机制层：100/1000个无关Go文件、25个消费者，预算五。最终38条样本由冻结版本重新生成，每次配置切换恰好重算25消费者、五批收敛且零源码重解析。100/1000无关文件下noop中位数20.206/25.520ms，配置续跑单批中位数20.148/24.046ms；每个规模只有三次noop和十五次续跑样本，不作为尾延迟认证。go_source_reads只计紧凑声明capture，不含scanner哈希、配置读取及提交复核。整体目录、包模型和闭包仍有随规模增长的成本。

检索层：P3-B已接受二进制对照最终P3-C，四套固定题库51题306请求，逐题Top1/nDCG差分无负值，四套指标保持一致；原始结果回放、14工具输入契约、两版本原Python/Rust签名十四表及独立CLI均通过。S11失败和三份compare inconclusive保留。一般语义效果、holdout、100k/RSS/尾延迟及发行认证不由该门推出。

## 格式与后续

当前schema17/model3，metadata key保留project_model_inputs_v1命名；旧缓存必须用隔离路径重建，不清空开发者日常缓存。输入确认仍在facts/frontier发布后，既不是OS原子快照，也不是所有后处理合一事务。watcher重试队列是进程内状态，不宣称任意崩溃下OS事件完整持久化。

Go为已准入、可移植、无条件、非测试文件集合；条件未选择时Unknown。MVS、多版本选择、version-specific replace、完整Go编译器/类型/作用域、vendor/remote安装和所有平台build profiles仍未认证。已知本地Go依赖精化不扩张成所有语言恒定复杂度保证。

收口后的下一批P3-D（P3-016～020）：配置深度/文件访问防护、import规模/缓存成本、清理旧解析重复逻辑、更新语言矩阵/配置契约以及G3完整阶段验收。首先复核本批源码/证据，不覆盖已有dirty工作。
