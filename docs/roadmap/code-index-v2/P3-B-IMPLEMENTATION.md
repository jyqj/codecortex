# P3-B｜捕获的 package 与多语言模块解析

状态：P3-B（P3-006～010）在声明的本地静态子集范围验收完成。累计70 done、122 todo；P3完成10/20，下一批P3-C。G3、完整编译器/运行时和发行认证未完成。接受版本仅为final-v3及paired-final-v3；前两次结果和修复过程全部保留。

基线 main@4514630dcd26481cf6dbc2aff38824ed71ef06da。本批入口核对 P3-A final-v2 的全部覆盖文件，无漂移；保留已有未提交工作及本批 entry-source.tar.gz。运行目录：`artifacts/benchmarks/p3b-20260928-oQGF/`。未提交、推送、PR、合并或修改日常索引。

## 实现与合同

| 任务 | 实际落点 | 能力与边界 |
|---|---|---|
| P3-006 | module_inputs.rs；project_model/package.rs；module_resolution/package.rs；JS/TS parser 与 imports 存储 | 捕获 workspace/package 入口，按声明顺序选 exports/imports 条件；持久化 static/require/dynamic/type-only 上下文；不读取安装依赖或运行用户脚本 |
| P3-007 | module_resolution/typescript.rs、package.rs、mod.rs | Node16/NodeNext、bundler、node10 的声明子集；扩展名替换、目录 package 优先于 index、ESM 显式扩展名；不伪造任意构建产物到源码映射 |
| P3-008 | project_model/rust.rs、rust_capture.rs；旧 cargo_workspace 模块仅测试兼容入口 | Cargo workspace/声明 path 依赖别名统一进入 ProjectModel；旧用例调用新模型，生产不再走旧独立目录扫描 |
| P3-009 | rust_modules.rs、module_resolution/rust.rs | crate/self/super、inline mod、外部模块与 path 属性、显式默认 feature profile；复杂 cfg/macro、平台、optional/dev/target profiles 保守 Unsupported |
| P3-010 | project_model/python.rs、module_resolution/python.rs | 静态 root/src、setuptools/Poetry roots、相对点、__init__、namespace；不执行被索引源码、初始化器或 import hooks |

导入上下文由解析器写入，四条 imports 写入路径及 dirty reload 均保留 context_json。ResolutionManifest 按 `(import_string, request_key)` 区分条件；不因同一说明符被 require 和 import 同时使用而丢失分支。模块决策与符号/调用置信度、新鲜度分别报告。

配置及 Rust 声明变化仍进入已有 ModuleConfig/负向依赖和持久 frontier。预算不足后可以续跑、重启，未完成之前不得报完整。配置捕获与纯解析分开；输入在提交前复核，确认在事实/frontier 成功发布后，不声称涵盖 OS 文件与全部后处理的单事务。

## 发现并修复的缺陷

`red-modules` 记录初始 package、Python 相对导入、Rust crate 路径三个实际目标失败。Python 相对导入现在不再复用 JS 扩展名探测。

`dev-modules-1` 发现两个单行/快照写入分支遗漏新增 context_json；补齐所有路径，未从十四表差分删除新字段。

`dev-workspace-1` 暴露失效导入仍残留旧 call_kind，造成全量/增量不同；现在同时归一化调用类别和目标状态。Cargo 声明 path rename 已支持，旧负例升级为必须得到 two/src/lib.rs 的真值断言，而非删除测试。

`red-edge-cases` 证明导入模块中不存在的成员会被绑定到其他文件同名全局函数。现在已声明导入查找失败会阻断该全局兜底。收紧后，旧 forwarding 用例暴露之前依赖同名猜测；新增有界的真实 PublicSurface 路径追踪，恢复 TS/Python/Rust 转发、别名和环的原始断言。缺失成员负例同时保留。

额外负例覆盖 package 源顺序、重叠子路径/null、合法数组目标缺文件不换下一目标、目录入口、scope 内重复包名、嵌套 workspace、路径越界/编码路径、namespace 与更后面普通包的优先级、Rust 模块双文件歧义及依赖条件限制。

## 验证记录

最终接受 `final-v3/validation.json`：26条验收命令均exit0。stable1.97.0与最低Rust1.95.0各自严格Clippy（全仓、全部目标、HTTP特性）、构建、完整测试及下表验证通过。初次 `final/` 功能通过但R04逐题质量未通过；`final-v2/` 的旧图分数公式断言失败保留，均不是最终接受版本。

| 验证 | stable1.97.0 | Rust1.95.0 |
|---|---:|---:|
| Workspace含doctest | 1589通过／0失败／39ignored | 1589通过／0失败／39ignored |
| HTTP特性 | 171通过／0失败／33ignored | 171通过／0失败／33ignored |
| 显式真实MCP | 16通过 | 16通过 |
| Watcher专项 | 13通过 | 13通过 |

另保留查询期间generation变化和单读连接并发验证，以及P3-B/P3-A/P2-D和SQL依赖工作量release测试。命令有重叠不能相加，ignored不计通过；这是本地macOS arm64，不是远端/跨平台CI。

final-v3 冻结535个源码/测试/运行文档/CI/脚本文件，摘要 `a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace`。路线图状态和历史 artifacts 单独核对，不写入循环哈希。初次摘要 `e0e7cbe74f2350c60cb84488a04ccb1e7f98bb06b2a84697174f8492874d7c13` 的原始证据保留。接受配对使用 `paired-final-v3/`；`scope-check-v3.json` 复核535文件内容与集合、26条命令日志哈希、前后版本二进制哈希、HEAD及14工具契约，均通过，不覆盖第一次原始配对。

真实 MCP 使用产品子进程与隔离 HOME/cache。package-only 变更、三消费者预算一：零源码重解析，每次正确目标 1→2→3，中间关闭并重启进程；前三步实际 complete 为 false、false、true。每种 evidence 层级见 `evidence-classification.json`。

接受版本release成本：122条package构建样本、34条Rust样本，含200000次纯查找，原始样本与统计见 `cost-summary-v3.json`。1k/5k无关TS文件下no-op中位数分别26.4525/35.3215ms，package续跑单次25.311/32.5395ms；每次discovery读取3个配置，提交复核另计。25消费者预算5，分批完成且不重解析消费者。100/1000Rust模块的no-op源码读取均0、声明缓存命中101/1001；捕获后删除磁盘源码再纯查询，单次中位1.03905/1.07695微秒。保留较早cost-summary.json为原始版本统计，不混入最终数字。这些是固定夹具机制测量，不证明全系统加速、RSS、p95/p99或100k。

## 逐题质量检查发现的排序回归

第一次配对，R04 Top-1由1降0、nDCG由1降0.5；R08刚好由0/0.5升至1/1，平均分因此相同。早期交流把均值不变称作逐题无退化是错误，已明确更正，并在 `quality-review/findings.json` 留证。后续不可只用平均分评定。

根因是Rust内联测试模块真实符号进入索引后，旧按文件先验抬高后的候选前缀选择图种子、再统一加连接度的路径形成正反馈。没有删除这些源码/符号或修改gold，而是让种子按精确身份与直接lexical/grep倒数排名选择，并用同一直接证据系数约束连接度加分；缓存policy版本同步更新。两项独立合成测试覆盖强文件先验不能买入种子优先权、零直接证据无连接度奖励与exact身份保护。

旧cc-server图排序基线保留：BM25项仍由原始SQLite值推导，新增图系数项用 `0.3 * ln(21)/10 * (0.5-1)` 独立手算，并保留原图分数、beta基线、最终顺序及确实发生图排序翻转的断言，而非重录新输出常数。

最终冻结配对再次确认R04恢复、R08提升，其余题目逐项无负delta，14题源码子集Top-1由0.8571428571升至0.9285714286、nDCG@10由0.9379235538升至0.9736378395。其他三套固定题库逐题一致：smoke0.7/0.7、exact1/1、intents0.625/0.65625。51道题、双版本、每题3次共306请求，raw回放一致，14工具输入契约、原Python/Rust签名十四表oracle和独立CLI重放通过。配对脚本逐项检测负delta并拒绝放行；S11无答案gate仍exit1，三份性能比较仍inconclusive。开发题用于发现修正，不能称为独立holdout或一般语义质量认证。

## 版本、未支持项与下一批

Schema16 与 schema15及更早缓存不兼容；ProjectModel payload version2，metadata key 保留兼容命名 project_model_inputs_v1。升级/回滚使用保留源码/二进制和隔离缓存重建，不清理开发者索引。

Rust 当前是静态默认 feature 和声明模块子集，不是完整 Cargo target/feature unification；旧无显式依赖的 workspace alias 便利能力保留 heuristic 策略。变更 Rust 文件仍有额外紧凑 tree-sitter 声明遍历；未宣称全链路一次 AST。无变化生产构建按内容 hash 复用，模块查找使用索引。源码目录、捕获元数据和相关闭包处理仍有随规模增长的成本。

完整语言约束、边界、预算和规范参照见 `docs/internals/MODULE_RESOLUTION.md`。S11、rolling R09 独立复核、公开 holdout、外部编译器、100k/RSS/尾延迟、Linux/Windows/远端 Actions、live OCE/embedding 与 G3/发行认证不因本批完成而通过。

下一批为 P3-C（P3-011～015）：Go module/workspace、统一模块结果、配置-only依赖精化、watcher重命名/溢出对账与模块规则外部oracle。复用现有捕获模型、依赖和持久恢复，不重造第二套引擎。
