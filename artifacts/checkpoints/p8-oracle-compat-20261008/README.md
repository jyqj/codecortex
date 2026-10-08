# 2026-10-08 PR 管理与 P8 多轮推进

本轮合并了 [PR #148](https://github.com/jyqj/codecortex/pull/148)，使 **7 个原任务完成**；随后修复了大表完整对比、兼容运行身份锁和转发预算计数。原任务总数仍为 **192**，其中 **160 done、32 未完成**（19 in_progress、12 todo、1 blocked）。本工件没有把工程修复、测试用例或新建协调 issue 计成原任务完成。

## 每轮计数

| 轮次 | 工作结果 | 新完成原任务 | 累计 done | 剩余 |
|---|---|---:|---:|---:|
| 本会话基线 | 固定 main `47d1939e43455ecd72ccc73e80825d971e3f33e7` | 0 | 153 | 39 |
| 第 1 轮 | 独立审阅、实际构建与原证据核验；#148 在精确 head 三套 CI 成功后合并 | 7 | 160 | 32 |
| 第 2 轮 | 完整大表 oracle、兼容输入锁及公开 DEV / 外部输入 / custody 缺口审查 | 0 | 160 | 32 |
| 第 3 轮 | 修复真实 10k 检查发现的 forwarding 预算错误，独立复核、回归与 PR 候选集成 | 0 | 160 | 32 |

第 1 轮合并提交为 `1fb51e181d6f7d4b15969d0bf362236ef6e06a58`，原 PR head 为 `9f6684ac81fa82f2bc99903ff42fb532478c2aaf`。完成的原任务是 P7-014、P7-015、P7-016、P7-017、P7-019、P7-020、P8-001。P7-019 / P7-020 沿用原任务 brief 的工程 / fake 与 live 分轨范围；P8-001 是原来固定的 DEV 输入和候选锁。它们没有授予新 P8 候选完整质量、clean holdout、真实 provider 或发布认证。

其余 43 个历史 PR 的 head、检查状态和采用范围已整理。对合并后的 main 再次进行祖先检查，43 个 head 均可读取，均不是该 main 的祖先；完整采用证据不足的 PR 继续保留。原审查及最新祖先检查分别保存在归档和 `merged-main-ancestry.json` 中。

## 三项实现修复

### 完整大表 oracle

`oracle::compare_streaming` 复用原 15 张表、原字段投影与 SQLite 值转换，在两个只读一致快照上逐行完整比较。临时 SQLite 覆盖索引负责排序，重复行保留，随后计算与原 JSON 数组格式一致的摘要。默认排序页缓存 2 MiB、每侧每表最多 500 万行、两侧累计 canonical 文本 4 GiB、临时库 8 GiB、单行 1 MiB；这些预算耗尽会返回错误。

独立审查发现并修复了 `-0.0` / `+0.0` 的文本比较回归：它们仍按原 `serde_json::Value` 语义判等，原排序文本与各自摘要保留。测试实际生成 **100002 行**，覆盖完整相等、尾部差异、重复行、全部表、四类资源预算、损坏输入和 FK 失败。超过原每表 100000 行界限的规模检查可以进入完整磁盘比较，未完成的索引 closure 仍不能报告认证相等。

### 兼容运行身份锁

`p8_compat.py` 现在核对完整 query / gold / annotations 等输入快照，以及 query-ID × repetition 的每个实际请求；同数量替换、未知 / 缺失 / 重复 ID、raw 路径复用、adapter / profile 漂移和退出码矛盾均拒绝。只补齐原 serde schema 的默认字段，没有改 scorer 或 gold。

原有四个 Express DEV 控制的 **774 个已留存请求**重新通过结构绑定检查。这不是 774 次新检索，也不是原 cc-switch / Flask 外部套件运行；原 `comparison_not_passed` / exit 1 结果继续保留。详见 [corpus / compatibility 工件](../p8-corpus-lock-review-20261008/README.md)。

### 转发解析预算

真实 10000 文件检查最初在首次 full build 触发 `reexport_file_budget_exceeded`。原实现把当前批次已经解析、已知没有转发的普通导入叶节点也计入 forwarding 加载预算。

修复仅排除当前批次 `Known` / `KnownEmpty` 且 `forwards` 为空的叶节点；它们的直接导出仍由原 symbol catalog 解析。未知 surface、需要数据库读取的目标、真正的转发节点继续受 **4096 文件、65536 边、32 轮**限制。初始、后续和最终深度检查采用同一过滤条件，没有增加配置层或提高默认上限。

## 实际执行及其范围

| 检查 | 观察结果 | 实际范围 |
|---|---|---|
| #148 原精确 head CI | CI / P7 engineering / P7 closeout 全成功 | 对应 workflow runs 37735492822、37735492668、37735492805 |
| #148 独立 release 构建和 stdio / P0 smoke | 通过 | Rust 1.95.0、`--locked`、真实 binary；1 道原 P0 DEV 题，不是质量泛化认证 |
| 新 oracle / defects / scale / route 回归 | 18 passed，0 ignored | 固定 `a20d269b`；其中新 streaming 测试 7 项；Clippy 通过 |
| P8 Python 脚本测试 | 221 passed，0 skipped | 包含兼容相关的 63 项及其中 16 个新增身份负控；这些子集不能重复相加 |
| 转发预算新回归 | 7 passed | 原测试提交的 4097 叶节点正控先失败，修复后通过；六类原界限控制继续通过 |
| 原解析 / SQLite 与模块规则回归 | 37 passed，2 个原 opt-in ignored | cc-eval P2C / P3B 18 项及 cc-index P3B 19 项；Clippy 通过 |
| 历史上下文 / adapter 控制 | 14 passed | 固定受测文件，114.002 秒；真实旧证明、失败注入、显式 fixture、嵌套 spies 与恢复 |
| 最终源码验证、174 项完整 source-integrity、fmt / plan / facts | **全部通过；174 passed、0 skipped** | 由独立验证者在远端固定 `a23bbfe8e16670a98ba32363bb419337522b8ba9` 执行；具体命令、状态和执行源见 `execution-summary.json` |

曾有一次 Cargo 命令写错不存在的测试 target，以 exit 101 结束；它保留在 `oracle-regressions.*`，随后正确命令的 18 项通过另存于 `oracle-regressions-corrected.*`。调试期间的 proxy 递归、源文件变动导致无效的草稿检查，以及中断的冗余 139 项专项日志也保留原状态。被中断的专项运行不是整套通过；最终完整验证使用原 CI 的 discovery 命令。

### 两次 10k 控制都没有完整通过

两次计划文件逐字节相同，SHA-256 为 `821d6ccea9509fd69a81ca08af7b780351ff11977e39eae2397efdd98128a52d`，均为 debug / smoke、10000 个文件、N=1、300000 ms 总预算。

| 运行 | 结果 | 保留的事实 |
|---|---|---|
| 修复前 `scale-10k-control` | exit 2，23.633 秒 | 第一轮 full build 触发 forwarding 文件预算；未进入 parity |
| 修复后 `scale-10k-forwarding-fixed` | exit 3，301.172 秒，`deadline_exceeded` | 两次 full build 各实际解析 10000 文件、生成 55620 symbols / 63619 chunks，parse_errors 为空；未输出完整 parity 或 run-completed |

复跑证明越过了原 forwarding 阻塞。它没有证明整个 10k 流程完成，更不是 1k–100k、30 次重复或性能认证。父报告保留时限、完整 stderr、binary 前后摘要和 fixture 清理结果；清理 error 为 null。环境明确禁用无法归属的 process probe，未知资源观测没有补成 0。

## 固定源码与独立审阅

- PRODUCT：`df140ad1dc97f080afd748e9571b360e0cade5fe`，tree `cc24dc3490aac7d52fc2b7568fd8341751fc84e0`。
- REVIEW：`b2a6c135444ef61d629cac1d739b120c8a3158d2`；[独立审阅原件](independent-source-review.json) SHA-256 为 `8a900584ff4ec4c3e86ba2a65b52a6d37909bb0c3bea66793448fbb9351e20e2`。
- 实际安装的 guard / registry / CI 提交：`a23bbfe8e16670a98ba32363bb419337522b8ba9`。
- 完整源码为 798 个输入，manifest SHA-256 `ef3fcf15da0523391b9c25abbc589b3c14498896eff581df416228619ef22aa6`；完整 validation 为 105 个输入。

每个源码 / validation 变更都记录作者和不同的实际审阅者。原 229 个历史保护文件、11 个测试模块 / 139 个方法体保持原字节。新 v14 先验证当前完整 validation，再在固定 `9f6684ac` 历史上下文执行真实 v13 双链及旧 CI 证明，最后核对完整 source / review 清单；新 helper、adapter 和测试本身都是被审阅的输入。

执行收据保留各次原始 commit，不把早期验证改写成在最终提交上执行。Git Data API 生成的远端 commit 与本地原提交身份分别记录。`p8-executed-source-history.bundle` 保留 9 个实际本地源 head，以已发布 `9f6684ac` 为唯一历史前提，可恢复 red / green 与整合执行使用的原提交。

## 原任务尚未结项的原因

| 原任务 | 实际缺口 | 协调记录 |
|---|---|---|
| P8-002 真实多仓 native 语料 | 目前 4 仓、301 native DEV 行；Rust / serde 与 mixed monorepo 覆盖及独立 gold 复核未完成。原 `09-BENCHMARK.md` D2 规划为 6 个以上真实仓库、初始约 600 个审阅问题；family 相关性统计不能替代实际审阅 | [#149](https://github.com/jyqj/codecortex/issues/149) |
| P8-003 外部兼容套件 | 原 cc-switch / Flask 输入尚未按仓库要求登记提供路径和使用方式 / 权限；固定输入上的实际计数、运行和 replay 未完成 | [#150](https://github.com/jyqj/codecortex/issues/150) |
| P8-004 clean holdout | 117 个既有候选中至少 72 已暴露，0 个获得 clean 认证；其余 45 个缺读者 / 首次可见性 / ACL 证明，需要独立封存及一次冻结运行 | [#151](https://github.com/jyqj/codecortex/issues/151) |

本轮另更正了派生审计中“600 个独立 family”的口径：原规格没有这个额外硬门槛；脚本里的 `formal_family_target` / `formal_family_shortfall` 不能替代原验收。历史测量保留，新增独立复核为 0 也不撤销已接受的历史 DEV 审阅。完整原文定位和更正收据见 [corpus 目标更正](corpus-target-correction/additive-correction.md)。

这三个 issue 映射原任务，没有增加任务总数。原 P8-002 → P8-003 → P8-004 依赖和后续正式验收继续有效，P7-018 的真实 provider 路径也未新增执行。因此本会话的“至少 10 项完全完成”目标尚差 3 项。

以上是第 3 轮结束时未完成的工作，不代表都需要用户补充授权。再次核对原 D3 决策后，公开仓库加固定 commit 的来源路线已获授权；原 `09-BENCHMARK.md` 的外部输入路径、使用方式和许可登记，也不能另解读为必须由用户再次手工提供文件。下一轮继续实际 source / gold 扩充和 canonical OCE 固定公开输入的本地准入与执行；clean holdout 仍须按原冻结、读者和一次执行要求核验。

## 证据文件

- `execution-summary.json`：本轮固定身份、结果和未完成范围的索引。
- `pr148-independent-validation.tar.gz` 与对应 manifest：#148 的独立构建、协议、P0、原 evidence 哈希及 PR 管理记录。
- `p8-engineering-validation.tar.gz` 与对应 manifest：新工程 red / green、Clippy、最终验证、10k 原 raw、完整收据和审阅材料。
- `p8-executed-source-history.bundle` 与对应 JSON：原本地执行源提交及校验结果。

两个归档均按原字节保存，每个成员有 SHA-256 / 长度，并在打包后逐项读回核验。体积较大的 binary / 临时 SQLite 数据库没有放入归档；构建命令、完整输入身份、实际 binary 摘要及原运行 raw 仍保留。撤销本 PR 时应整体撤回其源码和新校验选择器，旧失败及独立审阅继续通过 Git 历史保留。
