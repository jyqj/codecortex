# P8 第 4 轮：真实 DEV 语料、外部完整测量与 fresh holdout

本检查点承接已合并的 PR [#148](https://github.com/jyqj/codecortex/pull/148) 和 [#152](https://github.com/jyqj/codecortex/pull/152)。原始任务仍由 `docs/roadmap/code-index-v2/tasks.json` 管理；辅助脚本、审阅记录、测试和协调 issue 均不新增原始 TODO 完成数。

本轮目标是完成原始 P8-002、P8-003、P8-004 的数据、测量和封存工作。最终任务判定与固定源码独立审阅将另行记录。没有把测量得到的失败结果改成质量通过，也不认证完整 V19、G8、发布、100k 规模或真实 provider。

## 数据与角色

原先四仓的 301 条已接受 DEV 内容保留原固定 Git 字节、原锁和历史独审范围。本轮补充 Serde 150 条、Vite 149 条，分别由 `p8_corpus_closeout` 与 `build_validation` 编写、交叉全量独审，使用固定 F 的原生 CLI freeze / validate。总登记入口位于 `crates/cc-eval/benchmarks/public-dev-20261008/`。

600 是问题数。兼容投影不增加新问题；family 标签、共享源码/反例形成的关联组件以及重复执行也不增加独立样本数。新 299 题的独审不冒称重新人工复核了历史 301 题。新数据的审阅修正保留版本差异和原稿，不能据检索分数回填 gold。

fresh holdout 由 `pr_audit` 在固定候选之后编写，由非作者 `build_validation` 逐题复核。根代理在原始记录封存前仅接收元数据。它采用共享文件系统上的明确角色和访问记录；没有虚构外部保管人、ACL 隔离或未执行的能力。旧的已暴露或 custody 不明的 117 个候选不复用为 clean holdout。执行后的新 packet 已消耗，不能在后续调参后再次声称是未见样本。

| DEV 仓库 | Native 问题 | Compat 投影 | 无答案 | Family 标签 | 实际来源文件 |
|---|---:|---:|---:|---:|---:|
| Express | 70 | 59 | 11 | 70 | 7 |
| Requests | 91 | 83 | 8 | 91 | 20 |
| Gin | 67 | 55 | 12 | 67 | 53 |
| TypeScript | 73 | 59 | 14 | 73 | 20 |
| Serde | 150 | 149 | 1 | 118 | 60 |
| Vite | 149 | 149 | 0 | 138 | 148 |
| 合计 | **600** | **554** | **46** | **557** | **308** |

覆盖 10 类问题、Rust / JavaScript / TypeScript / Python / Go 五种核心语言及 Vite mixed monorepo；TOML / JSON / HTML 等额外文件语言单列。登记器实际调用固定 F 的原生 CLI 验证 20 套 native / compat 输入，全部 exit 0，原脚本与新增登记器共 21 项测试通过。旧 301 题按原 183 个 Git blob 锁复核，source.commit=null 的历史字段保持原样；新 299 题绑定真实固定仓库提交与逐题非作者审阅。557 个标签和历史 280 个相关组件都不是独立抽样数。

## 外部完整测量

外部 OCE 固定版本为 `d4f10554a18e31599d1e46d5d56da6588d4aa86c`。实际导入 cc-switch、Flask 各 100 条原题，分别使用规划中固定的上游源码提交。完整第三方题体、第三方执行代码及全部原始数据保持在仓库外的证据包中。此处只发布本项目实现、原执行身份和不含题体的校验摘要。

| 外部目标 | 输入文件 | 实际 indexed | Unknown | compat / native 测量条数 |
|---|---:|---:|---:|---:|
| cc-switch | 1,030 | 912 | 118 | 300 / 300 |
| Flask | 229 | 94 | 135 | 300 / 300 |

原 F 严格模式在 Unknown readiness 时中止，两套各 0 条 measured，exit 2；这些失败保留。新 D 评测器提供显式 `--collect-incomplete-coverage`：准备、协议、warmup 和原默认严格门不放宽，实际 Unknown 不改为 Ready，只收集全部固定请求用于诊断，而且即使诊断输入已 Ready 也不能返回有效测量通过。

实际四套共 1,200 条 measured，另按每题一次 warmup 运行。没有删除难题、缩小固定输入、修改原 gold 或重试检索。cc-switch 的缺口为 102 个当前 scanner 不准入的格式、16 个隐藏路径；Flask 为 118 个格式、17 个隐藏路径。同次隔离数据库的只读路径快照没有额外未解释路径。原始 4 套 gate 均为 `invalid_measurement` / exit 2。

| Profile | cc-switch Top1 | cc-switch nDCG@10 | Flask Top1 | Flask nDCG@10 |
|---|---:|---:|---:|---:|
| oce-compat-v1 | 0.240 | 0.253882 | 0.520 | 0.517282 |
| codecortex-native-v1（文件级投影） | 0.110 | 0.250204 | 0.460 | 0.514024 |

上表是完整分母下的失败诊断观测。compat 对路径去重，Top1 接受任一期望文件；native 只接受 primary group 为 Top1，按原始 hit ranks 消耗显式 alternative groups，因此不能混成同一排名。外部 native 投影没有编造 symbol、span、facet 或 graph gold。没有运行 OCE HTTP 服务，没有两个系统的实际同环境对照，不提供跨系统排行榜结论。

全部四套由原 F release `cc-eval` 在副本上 replay，重放后每套原有 319 个文件逐字节一致，原目录不变，原 exit 2 保留。参见 [汇总](canonical/summary.json)、[预先冻结计划](canonical/pre-run-plan.json)、[原始封存清单](canonical/original-seal.json)、[重放回执](canonical/replay-receipt.json)。

两份原题各 100 条都做了源码适用性复核。cc-switch 为 93 supported、3 disputed、4 uncertain；Flask 为 90 file-scope supported、6 scope notes、4 disputed。争议单独隔离记录但不删改原题或原分母。该复核不是把上游 gold 变成新的质量认证；Flask 收据明确披露了审阅期间根代理已看到 compat 结果，不能称盲审。

## 执行身份与验证

- 产品 F：`fb772551cff6b4620a6fcdb94c57b78350cebb33`，固定 release `codecortex` 和原 release `cc-eval`。
- 诊断评测器 D：`87ddafb409e4854baaa20ac339e97260d2fcb531`，固定 debug artifact。产品仍是 F，原 scorer 保持不变。
- D 的实际验证：52 个原契约/评分/锁/报告测试及 7 个新增诊断控制，共 59 项通过，Clippy 通过；独立完整源码审阅为 `accepted_scoped`。
- debug 评测器与 release 产品分别记录。没有冷 OS cache、正式尾延迟或性能认证；关闭的 process-tree probe 记 unavailable / null，未填 0。
- [执行索引](execution/index.json) 绑定原 source、命令、日志与退出码；不能把旧执行换成最终 PR HEAD 的执行身份。

## 留出集的实际结果

固定 F 后新增 walkdir / Click 两个仓库，32 个双语 family 共 64 条请求；N=1、warmup=0、固定 seed=2026100804，零重试。执行前的来源文件为 12 + 71 = 83 个，原适配器全部 Ready，冻结清单及原始数据的哈希在运行前后相同。

| 仓库 | 实际请求 | Partial | NoMatch | 原 gate |
|---|---:|---:|---:|---|
| walkdir | 32 | 28 | 4 | gate_failed / exit 1 |
| Click | 32 | 29 | 3 | gate_failed / exit 1 |

56 条正例的 Top1 为 10/56（0.17857143），nDCG@10 为 0.19053028；8 条无答案题全部失败。85 个返回片段来源字节有效，只证明出处，不证明检索相关性。按预先冻结的 2,000 次 repo 内 family 重采样，nDCG@10 描述性 95% 区间为 [0.08594426, 0.30964727]。数据只有两个仓库、七个源码关联组件，不据此宣称普遍效果或显著优越性。

所有原 raw 在读分数之前封存：107 个文件、1,887,925 bytes。原 F scorer 仅在副本上重放，两个副本逐文件一致，原始记录没有修改。非作者再次核对全部请求、原始 BLAKE3、83 个源文件、85 个片段及所有分组和预定统计。参见 [原结果](holdout/aggregate-results.json)、[分析报告](holdout/report.md) 与 [独立复核](independent-review/holdout-postseal-summary.json)。没有后续检索、改标签或按结果修复本次候选。

## 完整证据交付与原任务入口

原任务要求的固定运行入口见 [artifacts/benchmarks/p8-corpus-external-holdout-20261008](../../benchmarks/p8-corpus-external-holdout-20261008/README.md)。[交付清单](evidence-delivery.json) 绑定两个已经完整封存并保存的证据包：

| 包 | 大小 | 成员数 | SHA-256 |
|---|---:|---:|---|
| CodeCortex-P8-Holdout-Used-Evidence.tar.gz | 664,093 bytes | 261 | a5d5d8ff3844d28404b1cd49b728f88fb3a2e3b47472fa17a925b3a43e362f81 |
| CodeCortex-P8-External-Evidence.tar.gz | 53,696,127 bytes | 2,680 | 3b5b5260b7b1ef2ea3742a2fae80121fbb506b7210fa4d1f5ebee661062981d5 |

两个包都核验了每个成员的大小与哈希。external 包同时包含实际 F 产品、F evaluator 与 D evaluator 的三个固定二进制；完整题体、gold 和 raw 均保留在仓库外。来源投影不是完整 Git checkout，新严格运行需另取原固定公共提交。新增的封存后独立复核单独入库，没有重打包原 used holdout。

## 最终集成验证

本轮独立审阅的 P 为 `615662bd0e651dc40a9d1d0d6757bc28646b900d`，R 为 `a359581e6eb2ce8acc997d9c27103edd082e51ed`，安装审阅常量后的 S 为 `d5dfebd4f9add3694ec678830e6195c8d38b96f3`。842 个源码输入、107 个验证输入完整绑定；v14 仅更新四个引用常量和相应登记表，BASE、VERSION、FROZEN、排除规则、CI 及原 174 个测试方法都未变。

固定 P 的完整 Python 脚本回归通过 268 项、零跳过。完整 default cc-eval 首轮实际为 **398 passed / 2 failed / 57 ignored，exit 101**，原日志和固定源码前后清单保留在 [regression](regression)。两个失败分别为旧 Linux 进程采样器的命名空间归属问题、旧 fixture 的 index warm p95=547.91 ms 超过 500 ms。两个文件与原 BASE / F 均同字节。原记录不能以其他测试成功覆盖，也没有把满盘现象擅自当作两个失败的已证原因。

采样器缺陷另记 [issue #155](https://github.com/jyqj/codecortex/issues/155)。本次测量已关闭可选 process probe 并保留 unavailable/null，没有资源性能认证。最终原源码门禁、相关 feature 回归及标准 runner 的原 CI 终态分别归档；源码门禁通过本身不等于基础回归通过。最终原任务判定见独立 scope 审阅及 [PR #156](https://github.com/jyqj/codecortex/pull/156) 的固定 head 验收。

## PR 管理连续性

PR #152 已在其固定 head 的 3 个 CI workflow 终态成功后合并为 `804e56fe1cb2e1e2b7b9a1e879f84bda048c0a1b`。首次 closeout 中原 cold-client cancellation 测试失败的日志保留；有证据的 Tokio ownership 竞争只做了一次失败 job 的有限重跑。没有修改断言或增加无条件重试；确定性测试同步修复另记 [issue #154](https://github.com/jyqj/codecortex/issues/154)。

另有并行草稿 [PR #153](https://github.com/jyqj/codecortex/pull/153)，包含独立的 scanner 覆盖和输入身份工作。本轮未覆盖、合并或关闭它，其实现不冒称已进入本次固定 F 测量。旧 PR 的独立变更仍保留。
