# P8 本地工程 Rust delta 独立审查

**结论：accepted_scoped。** 本目录的 `review.json` 固定 base/source、逐路径 before/after
SHA-256、实际审查范围、引用的验证收据和结论。
base 为 `6d02d77f018a5965a6f289b0b43558ed4b9f8322`，source 为
`853385b7ccb2780818f9f8e8a83791f1c197efcf`，准确覆盖两者之间的 **17 个 Rust 变更路径**。
这是固定代码 delta 的静态接受，后续源码变化需要重新核对；不宣称组合 CI 已通过。

## 审查方法与作者边界

审阅者为本批 release/facts 子任务的独立 agent，未编写本次纳入的生产 Rust delta。
审查先完整阅读两个新驱动、入口及测试，再对照 base 阅读 measurement/gates 的全部
Rust diff 和必要调用上下文；固定 source 后核对全量变更路径及字节摘要。
执行 Cargo 测试的是集成者及各驱动实现者，原始结果按各自精确 source/binary 记录。
本审阅者不将阅读测试源码说成已执行测试，也不将被引用验证说成自己重复执行。

本收据不复审审阅者自己编写的 `p8_release_evidence.py`、`p8_facts.py`、Python 测试或
文档；这些项目由根 agent 单独审查。源码准入 helper/CI 的独立观察单列，不能扩充 Rust
scope 或替代原历史 source-integrity 重放。

## 已检查的行为

| 范围 | 具体检查 |
|---|---|
| 比较 CLI / availability gate | 比较输出通过 `create_new` 独占建立；无效输入保留 `invalid_measurement` 收据；空测量和缺失 p95 分母保持非零结果；已有质量与环境比较条件不放宽 |
| 延迟分层 | 所有 attempt 状态和缺失分母保留；warmup、重复和未版本化 raw hint 不推断 cache hit；完成样本 CI 与失败比例分开；binomial 秩的两侧尾与无穷端点表示核对；不把观测转为发布门 |
| 资源账本 | process tree 缺失、拓扑歧义和求和溢出返回 unknown；RSS 原生/ps 替代值、单进程/树和跨阶段峰值不相加；磁盘重叠身份、费用重复、货币和估算/报告口径分别处理 |
| 规模驱动 | 同 seed 两个实际源码树、真实 MCP index、完整原 oracle 和额外手写 config/fanout truth；不完整或超 oracle 行预算不能通过；全部重复保留；原始输入与 mutation 摘要、phase 报告保留 |
| 规模监督 | fresh process group、父进程 deadline、stderr 有界前缀、summary 有界读取、原输出拒绝复用；退出后的后代持 pipe 不再造成无限 join；停止后关闭 stderr sink，避免迟到输出改写已封报告 |
| 混合负载 | 有界队列和固定计划 offer；拒绝、晚完成 timeout、cancelled 与缺失终态保留分母；写入串行但允许实际 read/build 重叠；停写后先对账原增量库，不先修复掩盖差异 |
| 负载监督 | 监督前摘要、worker 当前可执行文件摘要与结束后摘要一致才允许正常测量；stderr 预算和 IO 错误产生 invalid；只监督自家可信 worker；其按名称查找可选程序的 PATH 指向本 run 的空目录 |
| route reload 修复 | reload 查询仅排除 `route_node:*` 派生行；核对其 StableId 前缀、派生节点生成与写入来源；parsed route 保留，原 canonical oracle 不更改，新回归同时检查公开调用、存储路由和完整 manifest parity |

## 审查中提出并处理的问题

1. 规模 summary 最初先完整分配再检查 128 KiB，已改为 `Read::take(128 KiB + 1)` 后拒绝超限。
2. 规模 stderr 最初直接 join，worker 后代仍持 pipe 时可能越过 deadline。已加入 fresh
   group 清理、原 deadline 内等待以及仅对完成线程 join，并加入真实后代持 pipe 反例。
3. 负载 worker 最初缺少自己的 binary 核对且 stderr 未限额。集成者要求的修复经本审阅者
   复读，包含三方 binary 绑定、64 KiB stderr 前缀及 observed/retained/dropped 收据，
   IO 或预算错误保持非零；空 PATH 改为专用绝对空目录，避免 shell 的 cwd 查找语义。
4. `CODECORTEX_CACHE_DIR` 可把索引写到自有 fixture 之外。规模驱动已加入清除该覆盖项；
   负载驱动清除继承的 CODECORTEX 环境项并由 worker 再校验，新增局部 CLI 环境注入与
   外部 sentinel 反例，避免全局修改测试进程环境。

## 已读取的执行证据与未验证项

| 证据 | 原始结果及精确边界 |
|---|---|
| round1 measurement | 新增 11 项和旧 report 11 项通过；scoped clippy 有完成日志；该组源收据为 bcce9efa；不是本审阅者执行 |
| 原 native sampler | 原组 7 通过 / 1 失败，隔离重跑仍失败；独立 Python 记录当前环境 PID 与 `/proc` 归属不一致，原测试未跳过或改绿 |
| round2 gates | 修复前 2 通过 / 4 失败；修复后 6 通过，加旧 report 11 通过；源码收据 b81d786d |
| route reload | 真实基线 parity 失败，修复后新例 1 通过、旧 DB route 7 通过；源码收据 462f7480，原 oracle 未改 |
| scale | 4917a43c 的 6 项合同通过，60 文件 smoke 共 9 个样本均通过；报告前后 binary digest 相同；f94e3205 的 CACHE_DIR 单行补丁只作静态复核 |
| scale clippy | 因共享磁盘耗尽被中断，exit 130；明确没有 lint 通过证明 |
| load / 最终组合 | 25399fc5 已静态复核并按最终 source 核对字节；该固定代码的最终执行结果由实现者/集成者另行归档，本收据不冒称已验证 |

引用的原始文件、摘要和源码对应关系见 `review.json`，完整文件仍保留在各自原始证据目录。
新源码已通过静态审查，与“全部测试/CI 已通过”分别记录，不能互相替代。

## 不能由该审查推出的结论

- 两个驱动使用本地合成项目和 in-process MCP。它们不证明产品外部 stdio profile、
  100k 完整 correctness、稳定 p95/p99、长时间稳态、跨平台或真实 provider 的表现。
- Hash 证明核对的字节一致，不证明 binary 的完整构建来源、不可变外部环境或任意 executable
  的可信度。特别是负载 supervisor 的 pipe 收尾只适用于它自己的 worker，空 PATH 不是通用沙箱。
- 原 native 资源探针的受控环境归属验证仍独立进行；此环境已有 `/proc` 归属异常记录，
  不能据短 smoke 宣称 RSS/CPU 归属与长周期峰值已认证。
- 原 P7/P8 依赖、V20、G8 和发布审批保持各自状态；`accepted_scoped` 是具体代码 delta
  的独立审查结论，不是这些父任务的 done 或发行许可。

## CI 保留核对

`ci-preservation-866d1b52.json` 记录首次纯加法检查。
`ci-preservation-9e03ed67.json` 随后固定最终 pin 提交
`9e03ed67a143dae03fa7fb2d862de29d7a254513`，逐字节验证相对 base 的 CI 变化恰为
release-evidence unittest、facts `--check` 两条新命令，以及
`p8-local-engineering-20261007-v8` 这个 source selector。30 个旧命名步骤和其余全部
字节/顺序均保留，包括 default/MSRV/security、optional HTTP、真实 MCP、模块/源码架构、
历史 corpus 和语义 stdio。最终 CI SHA-256 为
`1cf27f549d543b99ec3cdd0e5d4acfc6fa1cdf83a4a0088b0d81f42dbbd03fac`。

另核对新 registry 的 17 路径与本 review 的 before/after 摘要、显式 base/source/review
pin 一致，声明 777 个完整输入；旧 approved pins、历史 helper 列表与全局 base 保持。
准入函数 AST 与已独立审过的 98990b30 相同，仅 CI 预期生成/校验函数是已说明的加法。
这是固定声明的一致性检查，不等于完整 777 输入重放或全部 CI 命令已经执行。

原 `review.json` 保持 d07fceea 中的字节不变，它当时记录的最终 selector 待核对项由这个
后续 CI 收据补齐。后续执行验证和远端原始 review 对象可达性仍分别验证，不能由本地 Git
对象读取成功推断。

## 实际远端源码的独立绑定

`review-remote-source.json` 新增对已常规 fetch 的远端 source
`18499879a8ba3197e599cfd2e9ae96bcb657d65b` 的 `accepted_scoped` 结论，原
`review.json` 保持不变。独立核对时，本地远端追踪 ref
`refs/remotes/origin/p8-remote-source-20261007` 确实解析为该对象；对象唯一 parent 为
`6d02d77f018a5965a6f289b0b43558ed4b9f8322`，整个 base-to-remote 仓库 delta 恰为
原已审查的 17 个 Rust 路径，没有额外文件变化。fetch 由根 agent 执行，本审阅者在实际
对象可读后检查，而不把自己描述成执行 fetch 的人。

本次通过 `git ls-tree -r -z` 独立枚举本地与远端两棵受保护源码树，再用
`git cat-file --batch` 从两边读取全部 blob 内容。两个 source 的 **777 个输入、
9,972,292 字节**，逐路径 mode/type、Git blob ID 和实际内容均相同；SHA-256 全部重新
计算。`remote-source-inputs-18499879.json` 保留完整清单，摘要为
`f609895069a5ae658b8fce2516a7727b7ff36423d333e6a40a92afdca0627a12`。
17 项 before/after 摘要同时匹配原独立审查，777 项摘要也匹配本地最终 registry；原完整
canonical oracle 和仅加入两个 export 的合并结果再次核对。

远端 Git Data API 生成了新的 commit 身份；原本地合并提交的元数据和祖先结构没有被
复制。字节等价的范围是 `crates`、`Cargo.toml` 与 `Cargo.lock`，不能扩大成整个仓库树
相同：非 Rust 的工具、文档与证据会在后续集成提交发布。原 raw、binary digest 与测试
执行 source 均保留原值，内容相同不表示测试已在这个新远端 SHA 运行，也不补全 binary
构建来源证明。旧失败与中断记录不因发布而转为通过。

本审阅不读取或信任增量 Git bundle 来批准源码；bundle 如被保存，只承担原本地历史
的复放证据用途。最终 CI 必须从实际远端对象读取 R1 source 和含此 review 原始字节的
R2 review，并保留上述 CI 摘要、旧 pins 与全量历史检查。远端 review 发布和最终 pins
的核对单独留收据，不回写本次已绑定的 review 文件。

`remote-pins-and-ci-1c288cfa.json` 已补齐这一后续核对：实际 fetched R2 为
`c7766540612f1156f0a8afbdead20e934c98066c`，唯一 parent 是 R1，仅增加本次 review
JSON，文件实际 SHA-256 为
`c5e9f6811e348d075b893343eaf692e3e6425f5eb5bbb744319f638cb0605a1c`。
其内容与审阅者提交及本地最终 pin 提交
`1c288cfac90a4bff3d6cc162ca3531a1d21c6b25` 中的 review 完全一致。
再次读取 R1、R2、该 pin 提交三者的全部 777 个受保护 blob，路径、模式、类型、Git ID
与内容全相等，重新计算的 SHA-256 与 manifest/最终 registry 全部吻合。

准入 helper 相对前次已审版本仅替换 source、review、review path 与 registry digest
四组固定身份值；registry 仅改变对应绑定，原全部历史 pins、helpers、测试和准入代码
保持。CI 也重新从 base 构造预期结果后逐字节比较，恰好保留原 30 个步骤和其余全部
内容，只增加两条命令与 v8 selector，摘要仍为 `1cf27f54…03fac`。
这份收据检查的是实际远端 R1/R2 和本地固定的最终 pin 声明；最终完整远端树尚需在发布
后核对。R2 本身只有 review JSON，完整 manifest/README 随最终树发布，不能声称已经
包含在 R2 中。直接 guard 和完整 CI 的执行结果仍由集成者单独归档。

## CI 兼容修复的后续独立审查

`review-ci-compatibility.json` 固定接受后续远端 source
`40d54460f38ed13810f9898bc7257bc2cdd7fbad`（R3），base 仍为原
`6d02d77f018a5965a6f289b0b43558ed4b9f8322`，scope 仍是同一组 17 个 Rust 路径。
该 source 已由根 agent 实际 fetch；它的唯一 parent 为前次 delivery
`ae906513886bef4501d0a1d2ecffd68ef76c3f5d`，完整 tree
`02066a19d2107c195a88f0f84f90bbb7ad27b782` 与本地实现
`0ea1879417775ffb09c0fb5b2aae4f55c680b7ae` 完全相同。

本审阅者完整阅读新增 diff 和调用上下文，并从 base、R1、R3、本地实现读取受保护 blob。
完整 777 输入中，只有 `p8_load.rs` 相对 R1 改变，其余 **776 个输入字节相同**；原
17 路径中的其余 16 项 before/after 摘要均保持原审查值。新完整清单为
`ci-compatibility-inputs-40d54460.json`，包含 9,974,920 字节，SHA-256 为
`e6fae2ca81379ec5f95b50c84b5d69d99cd679941ad8750ace17db8b73591e78`。
两个原 review JSON 不覆盖，完整原 canonical oracle 与旧测试也保持原字节。

生产变化限于预算预留：将 `fetch_update` 替换成初始 Acquire load、checked addition、
固定上限判断及 `compare_exchange_weak(AcqRel, Acquire)` 重试。每次成功 CAS 是一次
完整预留；竞争或弱失败使用返回值重试，上限拒绝与整数溢出不修改计数。原 events/JSON
写入、错误文字、prefix 保留和 driver 边界均保持。两项新增测试分别直接检查 16 线程
争抢硬预算的完整预留计数，以及精确上限、零字节和 `u64::MAX` 溢出时的状态保持。

这是 `accepted_scoped` 静态接受，不是一次已完成的 CI 执行证明。收据生成时，原
Rust 1.99 弃用错误及 Rust 1.95 MSRV 结果来自集成者报告，原 job URL 已记录，但本
审阅者尚未读到归档日志；两项新测试和旧 load 测试正由实现者执行，未在 review 中写成
通过。后续原始验证收据保持各自 source/binary，不改写这些历史观察。

后续准入应将原 `p8_local_engineering` 同组 17 路径整体绑定到 R3 和实际发布的 R4
review，以 v9 selector 保留全部旧门禁，不新增重叠组。R3 是源码提交，其 helper、
registry 和 CI 此时仍保持前次 v8 字节；当前静态 review 不声称这些旧 pins 已允许新
源码。R4 实际 review 字节、v9 固定值和 CI 只改 selector 的核对需另行记录。
