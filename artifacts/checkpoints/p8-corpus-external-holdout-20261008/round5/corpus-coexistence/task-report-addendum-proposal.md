# 最终 3 项原任务的增补建议

本文件是给整合者的可直接采用文字，不修改 tasks.json，也不宣布尚未完成的最终合并。原任务的 steps、acceptance、deliverables、depends_on 保持原文。最终只追加证据、实现说明和准确状态；保留 PR #153 已有证据及其原作者、审阅者、提交和运行范围。

## 固定身份与采用条件

最终待整合源码 P 为 `1ed3c7df574db6d450f79ef29d5148f68556b3db`，树为 `d16aa044e56c9347e6b4ff75d783cc4317f15087`。此 P 的完整 benchmark 子树与先前独审的 `56d15ed246eedd9add0c0d2843001b5f4e971ee8` 相同，两个 registry、原 183 个 Git blob、我方 40 个新增输入和 PR #153 的 238 个 benchmark 输入均已重新核对。P 的产品源码不替换以下原测量身份。

本轮数据与一次性留出运行使用固定 F `fb772551cff6b4620a6fcdb94c57b78350cebb33`；完整外部 coverage-diagnostic 使用 F 产品和 D evaluator `87ddafb409e4854baaa20ac339e97260d2fcb531`。新合流 P 的检索质量仍为 `not_run`。PR #153 的原运行保留其自身产品、evaluator、输入集合和配置，不能与我方 F/D 系列互相替代。

在最终 P/R/S 和精确合并头的既定源码、回归及 CI 检查获得真实终态后，可按下列执行范围记录原任务完成。此前已留存的三条本地 Rust 命令 exit 101 及其独立归因/跟进必须一并保留；旧 S 的 174 项及 CLI 成功只属于旧 S，不能自动覆盖这些失败或代替新头的检查。本建议没有把 full V19、G8、质量或发行门槛改为通过。

## P8-002：建议追加的实现说明

保留 PR #153 的原 327 条公开 DEV 登记、原 source/gold 审阅及运行证据；该集合由历史 301 条和另一线程新增的 Serde 14 条、Vite 12 条组成。本轮另完成固定 600 条公开 DEV 登记：原 301 条保留历史审阅范围和 `commit=null` 快照，新 Serde 150 条与 Vite 149 条使用各自公开固定 Git 源、逐题非作者 source/gold 复核、显式来源与许可说明，以及真实 native/compat freeze 和 validate。新增 299 条的审核未利用运行排名调整题目。600 条覆盖六个仓库、十类问题及规划的 Rust、JavaScript、TypeScript、Python、Go 和混合项目范围；原文要求是“初始约 600 个审阅问题”，没有新增“600 个独立 family”条件。

两套 registry 保持各自冻结入口。按原 ID 及 Unicode NFKC、casefold、空白规范化检查，600 与 327 共享恰好 301 条历史题；这些共享题的题文、gold、范围、分类、语言及顶层 query_family/split 相同，推广副本只更新审阅 annotations 和已经审阅的关联映射。唯一并集为 **626 条 native 问题和 580 条 compat 投影**，其中 46 条 no-answer 仅属 native。不能将 600 + 327 记为 927；compat 投影也不增加问题数。本次没有重新冻结一个 626 条的合并 suite。

原 600 入口的 20 次固定 F validate 全部通过；合流复核又对两套既有入口实际执行 20 + 12 次同一 F validate，全部 exit 0，输入前后未变，没有 freeze 或 retrieval。源文件唯一并集为 309 个，131 个共享源文件字节相同。583 个 family 标签按已有相关关系合并得到 561 个已声明关联组件；它们不是独立样本。另有 20 个 PR #153 新 ID 与原 600 中的题存在 26 对 gold 代码区间交叠，已经透明列出，没有据此提高独立样本数或声称全语义去重。

该任务记录的是已审语料、输入锁和原生校验的执行完成。完整 600 条或 626 条在新合流 P 上的检索质量运行仍为 `not_run`，full V19/G8 和发行认证未通过。本线程的 Serde150/Vite149 审阅与 PR #153 另一线程的 Serde14/Vite12 审阅分别保存；相同角色名不代表同一位作者或本线程再次逐题审阅。

## P8-003：建议追加的实现说明

保留 PR #153 的原外部运行、原固定输入锁、独立审核和原结果，并保留其自己的产品/evaluator/configuration 身份。本轮另实际取得公开 canonical OCE 固定版本，在原 cc-switch 和 Flask 各 100 条题文与 gold 不变的条件下，分别执行 compat 与 native 四套固定输入测量。使用 F 产品及 D coverage-diagnostic evaluator，完成 **4 × 100 × 3 = 1,200 条 measured** 和 400 次 warmup。四套均保留原 `invalid_measurement` / exit 2，未把诊断采集伪装成可进入质量或排行榜比较的有效测量。

非作者审计已覆盖完整 ID × repetition 分母、unknown/missing/duplicate 检查、原输入与配置锁、所有 1,200 条 raw 的 BLAKE3/回显绑定、源覆盖路径及各次原退出码。cc-switch 实际覆盖 912/1030 个锁定文件，Flask 为 94/229；未覆盖路径的格式与隐藏路径归因单独记录。compat 和 native 保持独立报告，不能跨输入、平台 glob、模式或失效 gate 直接比较排行榜。canonical 上游 gold 的适用性争议如实保留，没有改原题、删争议题或改标签以抬高分数。

四套原输出已完整封存，在副本上用原 F scorer 重放；原退出码和失败 gate 保持，四套各 319 个成员、共 1,276 个成员与原件字节一致，没有重跑 retrieval。原 F 严格覆盖拒绝、D 诊断负结果，以及 PR #153 另一组固定配置运行彼此并列，不互相覆盖。新合流 P 的质量运行仍为 `not_run`。该任务记录外部套件执行、失效原因及可追溯证据闭环完成，不代表 full V19/G8、质量阈值或发行门槛通过。

## P8-004：建议追加的实现说明

在 F 产品和 evaluator 固定后，为 walkdir 与 Click 构造新的公开源留出 packet，按作者、非作者审阅者及执行者的角色记录保管过程；此保管证明是共享工作区中的角色与日志分离，不扩大为操作系统级不可访问证明。原历史 custody 报告及其已暴露问题继续保留，不能拿来替代这份新 packet 的状态。新增 64 条题由 32 组中英配对组成，完整非作者 source/gold 与翻译审阅在运行前完成，并按原元数据、分析计划、配置和执行锁封存。

固定 F 对两个仓的两套各 32 条请求只执行一次：N=1、warmup=0、retry=0，总计 **64 条实际请求**，随后封存 raw，再检查结果。原两套均为 `gate_failed` / exit 1，57 条 Partial、7 条 NoMatch；8 条 no-answer 全部失败。运行后没有改 gold、标签、题文或阈值，也没有为保分重跑 retrieval。原 scorer 仅在副本上重放，原封存件和失败结果保留。该 packet 已使用，不再称为未见留出集；新合流 P 不复用它声称新的盲测质量。

32 组中英题在 gold、类别、目标语言、scope 和 holdout split 上一致。已完成的 exact ID、family 标签及规范化题文零碰撞检查只覆盖当时的 **600 条 DEV**，不能自动扩写为合流后 626 条全体，也不能据 exact hash 证明不存在任意语义改写泄漏。两个仓和 7 个保守共享源组件限制推广范围，32 个 family 标签不是 32 个独立样本。零 ASCII 交集等词面统计只按原报告解释，不当作语义能力通过证据。

生产反过拟合边界检查绑定 F 的实际产品源与二进制：区分 cc-eval 评分端和 codecortex 产品端，保留完整输入扫描、实际生产路径检查、原扫描结果和负控。在所审固定生产输入与检查范围内，未发现产品注入 gold 路径表、题目关键词表或题体配置。此边界证据和一次性留出负结果支持原任务执行闭环；新 P 的质量、完整 V19/G8 和发行认证仍不通过或未运行。

## 对报告首页及最终回复的计数建议

本轮以 main 的 **160 done / 32 remaining** 为基线，只按唯一原任务 ID 计算。P8-002、P8-003、P8-004 三项在最终审核并合入后，主线应为 **163 done / 29 remaining**。加上此前已完成的 7 项，本会话累计完成 **10 项原 TODO**。

PR #153 分支先前显示的 162/30 是其中两项的并行提案，不能在合流时再重复增加两次。本轮添加的校验脚本、路径修复、证明收据、raw 包和审阅报告不计作额外原 TODO。门禁、规模或检索质量失败亦不计成通过；应与上述原任务的执行状态分别列出。

可直接采用的短段落：

> 本轮完成 P8-002、P8-003、P8-004 三项原任务的语料、外部执行和留出证据闭环。最终主线为 163 项完成、29 项剩余；本会话累计完成 10 项原 TODO。两套公开 DEV 登记去重后为 626 条原生问题，保留各自输入锁；兼容投影不重复计题。原外部 1,200 条诊断测量全部保持 exit 2，留出 64 条一次性测量保持 exit 1，8 个负例全部失败。任务执行完成并未将 full V19/G8、检索质量或发行门槛改为通过，新合流产品的质量运行仍为 not_run。

这段计数文字仅在最终真实状态达到该结果后使用；此前进度应继续显示当时 main 的真实剩余数。

## 既有仓库证据入口

- `crates/cc-eval/benchmarks/public-dev-20261008/index.json`
- `crates/cc-eval/benchmarks/manifests/public-dev-20261008.dataset-index.json`
- `artifacts/benchmarks/p8-public-dev-reviewed-20261008/projection-receipt.json`
- `artifacts/benchmarks/p8-corpus-external-holdout-20261008/index.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/corpus/registry-validation.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/canonical/results/`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/canonical/replay-receipt.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/holdout/execution-receipt.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/holdout/raw-seal.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/holdout/aggregate-results.json`
- `artifacts/checkpoints/p8-corpus-external-holdout-20261008/evidence-delivery.json`

本文件同目录下的 `final-P-data-identity.json`、`corpus-coexistence-independent.json`、`pr153-benchmark-byte-inheritance.json`、`registry-input-validation-independent.json` 和 `benchmark-entry-acceptance-independent.json` 是待整合者归档的增补审阅，不能先写成尚不存在的仓库路径。原路径缺陷与修复控制分别保存在 `package-boundary-original/reproduction.json` 和 `package-boundary-fixed/reproduction.json`；不改写原缺陷记录，也不把局部路径试验说成完成了真实发布打包。
