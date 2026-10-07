# PR 管理审计与执行快照（2026-10-07）

本检查点保存原始 **139 项 PR（#3–#141）** 的完整判定索引及清理结果。固定集成源已通过 [PR #142](https://github.com/jyqj/codecortex/pull/142) 保留 merge 历史进入 main。2026-10-07 执行快照记录：**93 项已关闭，其中 3 项随整合自动关闭、90 项在逐项复核 head 后关闭；失败 0 项，保留原 PR 46 项。**

这些数字描述此次已核实的执行快照。`index.json` 保存原始采用关系和管理建议，`cleanup-outcome.json` 保存执行结果；两者均不能替代后续对 GitHub 状态的读取，也不包含此次之后新开的 PR。

## 固定锚点与证据

| 项目 | 固定值 |
|---|---|
| 原 main | `ff458bc591b4e7e444af4464d6eef2513cdb335c` |
| 集成源（原 PR #141 head） | `886f90a542a6174a037c79eebbb4f74848fb1f53` |
| PR #142 merge | `8d6f38197c5a4431423833d99b7d77781daa965f` |
| merge 的两个父提交 | 上述原 main、集成源，顺序一致 |
| 新 CI | [37619378640](https://github.com/jyqj/codecortex/actions/runs/37619378640)，check / msrv / security 均成功 |
| 合并后的 main push CI | [37621679774](https://github.com/jyqj/codecortex/actions/runs/37621679774)，同一固定 merge 的 check / msrv / security 均成功；见 [回读收据](main-postmerge-ci.json) |

本地 Git 对象复核证明原 main 是集成源的祖先，且 PR #142 的两个父提交正是表中固定值。CI 结论来自集成负责人的 GitHub 回读；本检查点的复核脚本不重跑 Actions。原 PR 的历史 CI、此次集成 CI 和后续产品源分别计证。

## 文件与读取方式

| 文件 | 内容 |
|---|---|
| [index.json](index.json) | 完整 139 项索引：number、title、head_sha、base/base_sha、推荐分类、自动清理资格、推荐动作、准确理由和 Git 证明。 |
| [cleanup-outcome.json](cleanup-outcome.json) | 已核实的 93 条操作结果、46 条保留 PR 的开放快照、固定 merge 锚点；按主线程原始结果格式化保存。 |
| [pr139-ledger-history.json](pr139-ledger-history.json) | #139 唯一未匹配 ledger 提交的原始追加全文、before/after 哈希，以及集成后的明确接受段落。 |
| [reproduce.py](reproduce.py) | 只读重建固定 Git 证明、检查索引和执行快照的一致性，支持展开任一 PR 的全部路径差异。 |

原始矩阵约 1.18 MB，本索引约 195 KB。省略重复 API 字段和大批重复路径，保留固定对象、全部 original → matching commit 映射、逐路径结果计数、完整路径分组的 SHA-256 及未解决路径示例。复核脚本可以从固定 Git 对象重建所有路径，完整清理资格没有退化为“标题相似”或抽样判断。原始矩阵与审计说明的文件哈希也记录在索引中。

## 严格分类与操作含义

| 分类 | 数量 | 充分证据与建议 |
|---|---:|---|
| `exact_ancestor` | 47 | PR 固定 head 是集成源祖先，提交历史随 PR #142 merge 保留。 |
| `all_delta_paths_exact` | 31 | 以固定 base/head 的 merge-base 为起点，每条 delta 路径在集成源的 mode、对象类型、blob 相同；删除仍保持不存在。 |
| `all_unique_commit_deltas_adopted` | 14 | 所有非祖先独立提交均有完全相同的原始树差异映射，且无被忽略的 unique merge 提交。允许采用之后继续演进。 |
| `manual_adopted_product_ledger_evolved` | 1 | #139 的产品/证据已采用，ledger 需要下文明确说明的人工判断。 |
| `retain_requires_scope_review` | 46 | 完整严格谓词均不成立；索引逐项保存具体差异和不能自动关闭的原因。 |

前三类共 **92 项**，`auto_close_eligible=true` 表示存在充分采用证明。执行前仍必须确认固定集成源在当时 main 的祖先链，且 PR 仍开放、head 与索引一致。它表示可按已采用内容清理 PR，不表示需要再次合并旧分支，不认证旧 head 的 CI，也不要求删除分支。

实际清理中 #5、#123、#131 已随整合关闭，其余 90 项由主线程完成逐 head 复核后关闭，包含人工判断的 #139。`cleanup-outcome.json` 保留每项结果；复核脚本只核对这份快照与固定审计的一致性，不声称重新验证远端关闭操作。

## #139：产品相同，ledger 人工采用

[PR #139](https://github.com/jyqj/codecortex/pull/139) 固定 head 为 `e3b932ed4b1e197022c0902fd4c11af3e87ae87e`。

- **40 条非 ledger delta 路径相同**，包括 capture 产品实现、测试、契约及作者/独立审查证据。逐路径核对包含 mode、对象类型与 blob。
- **7 个 unique 提交中 6 个**在集成源历史具有完全相同的 raw tree delta；原始提交到匹配提交的完整映射保留在索引中。
- 唯一未匹配提交是 `063adeada4720b0fdab07493494bca0ebdcd47e2`，只修改 `tasks.json` 和派生的 `05-TODO.md`。权威 JSON 的唯一实质变化为向 P7-014 的 `implementation_notes` 追加作者阶段记录，任务状态和根 metadata 没有变化。
- 集成源的权威 ledger 使用审查后的明确接受记录，包含接受 `e3b932ed…` 的固定范围及保留原红色 partial-find 证据的说明。**原作者追加文字并未在集成 ledger 逐字保留。** 原追加全文已按原文冻结于本检查点，并附独立 before/after 字段哈希；集成接受段也完整保存。

因此 #139 的结论是人工确认同一产品已采用、历史 ledger 原文已另行保全。它没有获得整个 PR 的最终逐路径相同证明，也没有获得 7/7 原始提交差异匹配证明；`auto_close_eligible` 保持 `false`。

## 仍需处理的独立范围

以下五项特别保留独立未集成功能或诊断范围。该标记不声称旧 PR 的每一项改动都未采用。

| PR | 明确缺口 | 后续处理 |
|---|---|---|
| [#127](https://github.com/jyqj/codecortex/pull/127) | `IndexDb::publish_semantic_group` API、上限 4 的共享事务/lifecycle，以及 7 条新增测试/文档/证据路径尚未集成。 | 独立迁移审查后再考虑 production queue 接线；历史 CI 成功未证明性能收益。 |
| [#133](https://github.com/jyqj/codecortex/pull/133) | 独立 kind-taxonomy 诊断及 24 条路径缺失。Requests broad-function convention 有来源，Gin convention 未裁定。 | 推进评测字段契约，不能据此直接改 gold 或宣称修复已合入。 |
| [#137](https://github.com/jyqj/codecortex/pull/137) | 独立 Requests native-zero/qname 诊断及 18 条路径缺失，namespace 契约未闭合，原 head 历史 CI 失败。 | 按实际 qname/namespace 契约继续诊断；旧数据不认证当前质量。 |
| [#3](https://github.com/jyqj/codecortex/pull/3) | 旧非 embedding 索引/检索和质量回放功能，13 条路径缺失、32 条演进，非祖先；原 head 冲突且 CI 红。 | 逐项语义迁移审查，避免把旧 schema/benchmark 直接反向合并。 |
| [#4](https://github.com/jyqj/codecortex/pull/4) | 独立 AST facts、增量索引与 benchmark，21 条路径缺失、48 条演进，非祖先。 | 原 head 历史 CI 绿不等于完整功能已采用，仍需单独迁移审查。 |

其余保留项同样有逐项理由与可复核计数，并非因 PR 较旧而搁置：

| 范围 | PR | 未自动归并的原因 |
|---|---|---|
| 部分代码或 CI 已采用 | #10、#11、#111 | 尚有演进路径且完整提交差异未匹配，需细粒度意图审查。 |
| public DEV、corpus、准入、诊断 | #62、#64、#65、#68、#69、#70、#71、#72、#73、#74、#75、#76、#78、#79、#80、#82、#83、#86、#87、#90、#91、#95、#99 | 原始范围尚未完整导入；需按当前目标决定 intake 或证据归档，保留 source/review/license 绑定。 |
| 独立审查或迁移原件 | #104、#105、#106、#109、#115、#118、#120 | 产品修复采用不能替代整份独立审查证据的导入；负面审查也需保留。 |
| 固定失败测量或成本诊断 | #108、#112、#116、#124、#126 | 原始失败/诊断证据未完整导入；不能转述为当前性能通过。 |
| 候选及后续局部修复 | #114、#119、#122 | 候选实现演进和部分采用不足以证明整份 PR 已采纳。#119 的 shared-gate/NeedsRetry 阻塞有明确历史，不应为清理 PR 重新合并被拒绝候选。 |

## 复现

在拥有这些固定提交的完整仓库内运行：

```sh
python3 artifacts/checkpoints/pr-management-20261007/reproduce.py
python3 artifacts/checkpoints/pr-management-20261007/reproduce.py --paths 127
```

也可传 `--repo /path/to/codecortex`。脚本只读取本地 Git 对象和本检查点，不修改工作树、不访问 GitHub API、不执行合并或关闭。缺少固定对象时会失败；需先从仓库取得索引的 head/base 和固定锚点，例如抓取对应 `refs/pull/<number>/head`，必要时抓取记录的完整 SHA。后续 PR 被改写时仍以索引的固定 SHA 为准，不能用新 head 替代。

默认运行重建 139 项分类并逐项比对证据、核对 PR #142 merge 父提交、复核 #139 的原 ledger 追加和集成接受段，并检查清理快照的 93/46 分组及固定 head 一致性。`--paths N` 在完整复核后输出该 PR 所有路径的 exact/evolved/missing/deleted 分组。

本次审计覆盖 PR 采用关系、来源与管理顺序；没有以清理结果重新认证全 P7、V19、真实 provider、holdout 或规模性能。原始失败记录及独立范围继续按其固定来源解释。

## 清理后的待审标注

[retained-followup.json](retained-followup.json) 记录后续管理动作：#3/#4/#127/#133/#137 已补充具体保留理由与迁移入口；#3/#4 转为 draft 待独立迁移审查，其余三项原本为 draft。上述 head 均在动作前重新核对。原 cleanup-outcome.json 是此前的操作快照，保持原样。
