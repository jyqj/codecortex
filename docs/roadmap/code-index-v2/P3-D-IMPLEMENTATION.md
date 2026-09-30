# P3-D｜统一模块入口、防护与 G3 验收

状态：P3-016～P3-020及G3在本页声明的本地静态能力范围内验收完成。接受final-v3；2026-09-28续接核对555个覆盖文件与已完成的双工具链证据，补完固定题库配对、独立复验和任务表收口。累计80done/112todo，P3完成20/20，下一批P4-A。不是完整编译器、跨平台、规模或发行认证。

基线 `main@4514630dcd26481cf6dbc2aff38824ed71ef06da`。入口核对 P3-C 的548个覆盖文件内容和集合均无漂移；已有dirty源码和文档保留于本批 entry.json/entry-source.tar.gz。运行目录 `artifacts/benchmarks/p3d-20260928-oO9a/`。不提交、推送、PR、合并或清空日常索引。

## 实现与实际问题

P3-016：应用设置、模块配置、紧凑源码后备读统一为 `cc-model::input_file` 有界普通文件读取。原 `config_cache` 在 File::open 之后才判断普通文件，FIFO tsconfig 会无限阻塞；`.codecortex.json` 的 read_to_string 有同一问题。两次修复前子进程测试以3秒截止失败并杀死子进程，原失败记录 red-boundaries/red-own-config 保留。修复后 Unix 逐级描述符相对、no-follow、nonblocking、前后普通文件校验；其他平台只有较弱检查，不宣称同等竞态保证。路径 API 仍是验证时检查，不冒充句柄读取的原子性。

P3-017：JS workspace/Python root 按祖先键查找，Rust alias 通过 entry-owner 索引查找而非遍历所有 crate；歧义不取第一名。Go保留所选workspace实际依赖的成本；复用已有4096成员上限并补充有限候选预览，不把已有上限说成本批新增。新混合多包release夹具分开记录扫描、模型捕获、真实构建/缓存、纯模块查找，删除整个源码树后继续检查实际目标；不是全局 syscall 计数或constant-time证明。

P3-018：删除 `cc-parsers/src/import_resolver.rs` 的磁盘解析器与JS/Python通用探测公式；SymbolCatalog不再重复生成路径负依赖。实际重现了 Java ./helper 误命中 helper.ts，现由统一模块入口返回Unsupported。Vue/Svelte已经提取的script导入复用JS/TS规则，其他语言不再借用错误后缀。旧三项辅助测试由新独立模块目标/越界测试覆盖，原源码归档保留；这是内部Rust crate源码接口清理，不是删除MCP工具。

P3-019：增加 MODULE_CAPABILITIES.json 与 check_module_architecture.py，校验真实schema/model常量、文档/测试入口、删除路径和模块目录内有限词法IO禁用检查。该脚本不冒充完整静态程序证明。配置文档明确应用设置失败的默认值+环境覆盖及告警；config_linker不是模块解析器。

P3-020：新增四语言混合实际parser/SQLite独立目标断言、noop/reopen十四表一致、真实MCP八个具体调用目标。保留P3-A/B/C全部配置-only、预算恢复、条件/负例、watcher重新扫描和失败重试测试；跨版本固定题库继续逐题比较，不以均值抵消退化。

## 冻结复核中的本批修正

第一次 final 已通过 stable 全仓/HTTP/MCP及MSRV严格检查，但作者复核发现 Go成员限额已存在，本批又插入一层相同判断和take，导致候选预览分支无法到达。已主动停止该次Job，原验证JSON、日志、源码摘要和停止事件保留，不将它当作完整验收。新的 red-preview-witness 断言精确要求64个诊断预览，实际失败0/64后合并为一个判断；没有通过删测试隐藏问题。这一处合并已通过补强后的回归。

进一步复核配置分类发现既有 `ends_with("package.json")` 把 base-package.json、subpackage.json 等普通TS继承配置误认作包清单，合法别名变为Unsupported。red-config-basename先实测失败，随后改成精确末级文件名判断。final-v2同样主动停止、原记录保留，最终只接受final-v3完整重跑及配对；已有package入口测试不删除。

## 格式与边界

数据库schema18要求从17及更早隔离重建，以清除旧猜测与依赖。模型布局仍为3、metadata key仍为project_model_inputs_v1。Unix根是用户授权的可信根，别名可用；本批不承诺对恶意挂载、硬链接、根祖先变化、已打开目录迁移和所有源码路径TOCTOU的全局沙箱保证。新读取函数不执行输入代码、不联网。

Go为可移植无条件非测试包子集；未知cfg/cgo/环境不会变成成功。其他语言也仅是文档声明的静态子集。外部模块参照必须以本轮实际版本与结果为准，没安装工具的not_run不转为通过。公开holdout、100k/RSS/尾延迟、完整compiler/runtime、Linux/Windows/远端Actions、live embedding/OCE和发行认证仍不由G3代替。

## 最终验收与本次续接

唯一接受的源码摘要：`e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461`；555个覆盖文件，文件集合与内容均无漂移。`final-v3/validation.json`中的28条Cargo命令全部退出0，日志与产品二进制哈希已经逐项复核。此前停止的final/final-v2没有被改写为成功。

| 检查 | 本地stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 全仓严格Clippy，全部目标及HTTP特性 | 通过 | 通过 |
| Workspace及doctest | 1624 passed / 44 ignored | 1624 passed / 44 ignored |
| HTTP特性 | 176 passed / 37 ignored | 176 passed / 37 ignored |
| 显式真实MCP | 18 passed | 18 passed |
| watcher专项 | 17 passed | 17 passed |

本次续接没有重新宣称上述全仓命令全部在新会话执行，而是确认其精确源码、日志和二进制仍一致，再执行版本配对及补充复验。`checks/resume-HjT7-boundaries.json`为11passed/1ignored，ignored是由父测试显式启动的FIFO子进程入口；`checks/resume-HjT7-mixed.json`为2passed/0ignored，包含四语言真实MCP目标验证。新旧命令覆盖有重叠，不相加。

`scope-check.json`核验28条命令及日志、14个工具输入契约、原Python/Rust十四表变更对照、独立CLI和两个工具链的四语言8个具体调用目标。`scripts/check_module_architecture.py`同时通过；该检查只证明其声明的有限源码/文档一致性，不是完整信息流证明。

## 固定题库、成本和外部参照

P3-C/P3-D四套51道固定开发题完成8组306次请求，逐题Top-1和nDCG差分全部为0；原始结果回放一致，无无效源码命中。源码子集Top-1=0.9285714285714286、nDCG@10=0.9736378395408184；S11无答案测试仍失败，三份性能compare仍为inconclusive。`paired-final-v3/summary.json`保留逐题结果及原始失败，没有修改gold，也不是独立holdout。

新增50条release机制样本：每种语言10/100个包，共40/400个语言项目；分别记录实际全量/无变化构建、扫描、模型捕获和40000次纯模块查询。纯查询在整个夹具源码/配置树删除后仍断言具体目标。结果见`cost-summary.json`和原始`observations/final-v3-p3d_cost-release/p3d-import-cost.json`。

| 每种语言包数 | 总语言项目数 | 无变化构建中位数 | 扫描单样本 | 模型捕获单样本 |
|---:|---:|---:|---:|---:|
| 10 | 40 | 28.285 ms | 3.569 ms | 5.764 ms |
| 100 | 400 | 85.519 ms | 12.214 ms | 59.390 ms |

100包/语言时，TS/Python/Rust/Go纯查找中位数分别为1.955/1.075/1.087/2.150微秒。查找不逐次访问磁盘不等于整个构建无I/O：无变化构建仍读取42/402个配置输入并复用解析，发布前还单独核验。扫描/捕获为每规模一个样本，不能作尾延迟或整体加速结论。旧P3-C/B/A/P2-D成本独立重跑，不与这50条相加。

同一组四个Rust/Python外部案例在两个工具链运行中通过，版本/输出/fixture/harness哈希保留。两个Go外部案例因工具未安装为not_run，TypeScript编译器未执行。外部工具可用范围的通过不能替代缺项。

## G3范围与后续

G3只放行已声明的本地静态模块链路：配置捕获与规则分离、类型化结果及依赖、配置-only恢复、四语言实际目标、访问预算/路径防护、旧通用解析器移除和成本可归因。该结论依赖P3-A/B/C回归在同一final-v3中的重跑，并不放行完整语言语义、真实Go/TS编译器一致性、Windows/Linux、100k/RSS/p95/p99、公开holdout、全查询快照、live embedding/OCE或发行。

下一批P4-A（P4-001～005）：SourceSnapshot/原始bytes与半开span、复用已有AST的紧凑边界、类/方法层级切块、长函数statement/block分割、注释与签名关联。先保证CRLF/UTF-8/末尾换行的独立原文与位置真值，再改chunker；不要为切块另起一轮全仓解析，不提前把新块身份当成已验收的向量版本。

续接还发现PLAN-CHECK.json落后于tasks.json（70/122而非75/117），收尾必须由权威任务状态重新生成；不能用旧计划检查当作当前工程通过证据。
