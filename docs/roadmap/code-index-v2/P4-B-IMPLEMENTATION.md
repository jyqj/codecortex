# P4-B｜同域切块合并、组合预算与独立模型输入

状态：P4-B（P4-006～010）已按最终冻结源码完成本地声明范围验收，90done/102todo；P4完成10/20，G4未完成。

基线 `main@4514630dcd26481cf6dbc2aff38824ed71ef06da`。入口核验P4-A的565个覆盖文件无内容漂移，原有dirty工作及任务文档归档在 `artifacts/benchmarks/p4b-20260928-tuNL/entry-source.tar.gz`。本批不提交、推送、PR、合并或重建日常索引。

## 实际问题与修复

`red-policy-fragments`在原实现上产生三个独立失败：项目配置3行仍产出23行源码块，新增64字节字段未被配置/生产解析器消费，150条语句函数被拆成154个检索候选。固定原件、原始失败与后续通过结果分别保留；前两项是接线缺口，第三项是碎片数量回归用例。

P4-006：在已有Partition输出上增加同owner/同结构父域的相邻小片段合并；不跨独立声明、文档和不相关控制域。不生成重复父类全文，已实现的区间排序/裁剪复用而非另造切块器；重复、包含和交叉span的原文覆盖有独立断言。

P4-007/008：所有symbol/gap/tail与无AST输入使用同一个Budget和UTF8/CRLF安全fallback。畸形语法可返回显式partial边界，致命解析/编码错误仍是错误或跳过，不伪装成功空文件。极小预算、非ASCII单行和中间位置片段继续保留原始字节proof。

P4-009：ChunkPolicy固定版本、单位和边界；项目indexing设置贯穿全部Registry成员和SFC。行/字节/Unicode标量/估算token与合并阈值均有范围检查；估算单位仍是ceil(bytes/4)，不冒充精确tokenizer。每次构建捕获indexing配置，单个长期MCP进程也能改预算。`files.chunk_policy`与每文件chunks同事务提交，并纳入file_state聚合版本4和缓存；规则变化不再被mtime/hash fast-skip掩盖，事件路径外的旧stamp会触发全树准入扫描。失败文件保留旧stamp，成功兄弟文件不代其确认；没有增加平行迁移队列或全局确认位。配置在prepare期间改变需下一次build收敛，不是OS/全查询快照。

P4-010：`documents::render`直接消费已核验SourceSnapshot/ChunkRecord，分别生成含路径、语言、owner breadcrumb、签名的模型输入。原始chunk.text不变。渲染的原文范围、实际输入hash和配置敏感render_key可以独立检查；元数据截断显式，正文不够预算则失败要求重新切块。这里是实际可调用并以真实解析块验证的纯API，还没有provider调用、异步任务、DocKey/Version或持久document manifest；后者属于P4-C/P6/P7。

准备/提交边界复核另发现本批新增报告字段可被另一套Indexer预算错误标记：`red-policy-transport`已实际复现。PreparedBuild现携带准备期policy，提交前检查一致性，报告沿ReportCarry传递原始policy，而非从后续Indexer重新推测。失败时文件事实保持原样。

## 验证范围

开发测试覆盖96组组合预算/语言/Unicode输入，固定声明/父域边界、原文与坐标、live配置修改、解析失败后恢复、restart/no-op与全量十四表一致。独立real-MCP测试和release合并消融必须以最终实际收据为准。完整旧workspace、P4-A源码、P3模块/配置恢复、P2增量、14工具输入契约与固定题库配对都纳入最终验收。

最初复制check.py遗漏entry环境字段，修复前没有启动Cargo，记录为harness问题；开发Clippy指出算式风格与release-only测试常量断言，分别修正/说明。源码架构检查的SFC文本匹配曾不接受rustfmt换行，随后改为有界正则；不是产品接口绕过。

开发配对曾出现R12首位退化，其余题目无差分。原因不是源码证据错误，而是本批把同一父块下的多个独立if单元当普通小语句合并。`red-control-barrier`使用独立命名且有/无大括号的分支复现此问题；新增Control边界并传播到包装statement，独立控制流单元不跨组拼接。没有按题名、路径或查询设特殊规则，也没有修改gold；最终配对另行判定。

## 格式与未完成能力

schema20，ProjectModel3，file-state aggregate4。旧19及更早索引隔离重建。Chunker旧公开line_budget字段改为内部完整policy；项目配置及公开MCP仍向前保持14工具输入，新增报告说明实际build policy。源码/模型文本预算分开，不能把源块cap当整个MCP响应或进程内存cap。极小合法预算会增加块数和proof开销，后续资源/规模认证仍需测量。

G4、持久文档身份、provider、查询时完整磁盘freshness、holdout、100k/RSS/尾延迟、跨平台CI与发行认证未完成。最终结果见下节冻结验收记录。

## 最终验收结果

575个覆盖文件、30条Cargo命令、两工具链严格检查/完整测试/真实MCP均通过；固定51题306请求无逐题负差分，来源证据无错误。接受`final`及`paired-final`，源码摘要`ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb`。

| 工具链 | Workspace | HTTP | 显式真实MCP | watcher |
|---|---:|---:|---:|---:|
| stable | 1661 passed / 48 ignored | 186 passed / 41 ignored | 20 passed | 17 passed |
| 1.95.0 | 1661 passed / 48 ignored | 186 passed / 41 ignored | 20 passed | 17 passed |

命令之间有重叠，不相加；ignored不计通过。本地macOS arm64，两工具链版本、SDK、命令与日志摘要见validation.json，不代表远端CI。

成本结果见cost-summary.json和原始30样本；比较同一源码/parser仅切换merge阈值。全局吞吐、100k、RSS和真实tokenizer不是此消融的结论。

所有原始失败保留；最终scope-check核验源码集合/内容/日志/二进制/旧输入/配对。下一批P4-C，G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。
