# P8-004：Boltons prospective temporal-v2 封存评测

## 验收结论

独立最终审查者于 **2026-10-08 12:22:38 UTC** 完成原任务核对，接受本次证据为 **原 P8-004 程序范围的补充验收**，该范围的未解决阻断为 0。详见 [最终人工验收](workflow-control/reviews/P8_004_manual_acceptance.json)：28,948 字节，SHA-256 `ad746bdbd2438f61ada6dc2b08300655980e2d5d8a77023876c2deb165af2f38`。

正式候选仍为 **gate_failed / exit 1**；成功的只读回放没有改变这一质量结论。各审查者的实际角色、早期设计/源码参与和相互复核范围在回执中逐项披露，未声称每位审查者均与整个候选源码毫无作者关系。

本次是原任务 P8-004 的追加证据。PR #156 已于 2026-10-08 合入主分支，将原任务完成数更新为 163 / 192；本补充保留其全部源码、600 条公开 DEV 语料、独立审查与台账说明，不新增任务、不重复计数。剩余任务为 29 项。

## 真实测量结果

固定候选为 `2cd04485b0e0b23483f64671d56538bbaa47f442`，上游为 `mahmoud/boltons@4e5faa3d7e4008d89e0d8bf1ea87b6d9a061a16d`。实际 run [37769636440](https://github.com/jyqj/codecortex/actions/runs/37769636440) 仅有 attempt 1；原始 Actions 结论为 failure，原 helper 退出码为 1。结果没有替换、删行、重标或重跑。

| 指标 / 范围 | candidate_local_default | rg_baseline |
|---|---:|---:|
| 原生题目 | 48 | 48 |
| 实测记录 | 144 | 144 |
| 有答案记录 | 120 | 120 |
| 有答案 Top-1 | 0.175 | 0 |
| 有答案 nDCG@10 | 0.1891629278415609 | 0 |
| 有答案 MRR@10 | 0.22083333333333335 | 0 |
| 无答案正确率（24 条记录） | 0 | 1 |
| Partial 状态 | 141 | 0 |
| NoMatch 状态 | 3 | 144 |
| invalid / unverified hits | 0 / 0 | 0 / 0 |
| 原 gate | gate_failed / exit 1 | baseline_recorded_not_quality_certified / exit 0 |

表中的 `candidate_local_default` 是冻结臂的名称，指本地检索和 default-feature 构建。该臂实际使用显式索引配置：`include_text_files=true`、`include_hidden_files=true`、`auto_index.enabled=false`；不把它解释为原始默认索引配置的测量。

48 条题目来自 12 个原始任务，每个任务有英文原句、中文翻译、英文改写、中文改写四个版本；每臂重复 3 次。统计保留全部 288 条结果。回答质量按 10 个有答案来源组件与 2 个无答案组件分别统计，变体与重复次数不当作独立样本。原生 Rust 报告及预先冻结的组件配对 bootstrap 报告同时保存，不混用两份报告的置信区间。未定义指标保持 null/inconclusive，未缩小分母。

rg 在该固定自然语言查询方案下全部返回 NoMatch；这份小样本基线的差值不能证明普遍检索优势。候选的无答案正确率为 0，原质量门没有通过。该测量不能认证后来 PR #156 合入的 `1ed3c7df…` 产品版本，也不授予 G8、真实语义收益或发行认证。

## 封存与信息流

1. 完整候选源码 1,038 项、扩展验证输入 110 项、真实 release 包、二进制、配置、scorer、48×2×3 顺序和运行依赖先行冻结。正式候选和测量 helper 在首次封存后保持不变。
2. 2026-10-08 10:18:10 UTC 首次封存发布，10:19:23 有独立服务观察。随后才以 `fork_turns=none` 启动新的题目作者和独立 gold 审查者；实际首次时钟分别为 10:21:50、10:21:57。
3. 作者初稿于 10:32:13 写入并回读。运行所有者首次读取完整题文发生在 10:33:22–10:33:39，晚于候选封存。一次基于固定源码行为的题面澄清修订发生在数据封存前，只澄清 f0008 的英文原句及中文翻译；gold、其余 46 条题目和生产源码没有改变，原稿、修订稿和独立审查记录均保留；正式运行后没有修改标签或题目。
4. 独立源码/gold 审查接受 48 变体、22 个 gold span、10 个 hard negative、2 个完整缺席作用域和 12 个任务组件。另一个角色对许可的 327 条公开 DEV 作有限关系审查；未知历史、未授权旧题文和预训练内容不声称已排除。
5. 原生 Rust freeze/validate 是真实零查询准备运行，确认 48 题、113 个准入文件；唯一 suite 改动为原生 queries_digest。完整原件见 [实际准备档案](actual-native-preparation/37766434808/root-export-manifest.json)。
6. 真实数据封存发布于 11:13:36，独立观察于 11:20:53。只有取得当次服务身份和独立释放记录后，11:25:50 才首次发布绑定 run/attempt/job/utility 的一次性许可。原许可 ref 没有更新；正式执行仅消耗此一次机会。

固定 data inventory SHA-256：`f90b9a9667d045786ccd5e951186c7cf260e8935a5b34ca7959dccde6f958f59`。固定 attempt ID：`36e3c9f404428e8e90ada105489a184478c753223b85de34c342d2b41639a6e2`。

上述隔离是有原始记录与独立观察支持的操作约束，不声称具有系统 ACL 级别的不可访问保证。旧 117 个候选中至少 72 个已暴露，原 clean 认证数仍为 0，其余 45 个的未知 custody 不能推断为 clean。当前题目经过本次评测后已退役，不能作为调优后的新候选的未见 holdout。

## 原件、回放与独立审查

[正式执行完整档案](actual-execution/37769636440/export-manifest.json) 保存 1,156 个导出文件，包括完整原始 ZIP 的 31 个分块、每个 ZIP 成员的 inventory、原服务元数据和精确 UTF-8 副本。原主 ZIP 为 23,093,019 字节，SHA-256 `8158bf53ed937017e75c37c7ad183d60806964283d236a4bccd37e9917aa89eb`；原共享 Rust ZIP 为 1,501 字节，SHA-256 `3517b0863a4f4dcd0c5ecb47d0553359cfcfe5192498d8a703689ce3e220457b`。

独立原件审查于 2026-10-08 11:54:44 UTC 完成，原审查文件 [actual-raw-input-review.json](workflow-control/reviews/actual-raw-input-review.json) 为 15,139 字节，SHA-256 `bd233a4d4c35e02dc68269d4a69f3049a48fcb4e2656a402cb785d5dca49a2ee`；[审查附录与实际复核代码](workflow-control/reviews/actual-raw-input-review-appendix.json) 同时保留。审查独立解压主包 1,114 个成员、Rust 小包 1 个成员，核对全部 CRC/成员 SHA/模式，重算 323 条 runs inventory、288 行顺序与 252 个命中证据，并核对 9 项固定统计的 bootstrap。原运行中的 scorer 回放副本和后续独立服务回放分别记录。

原 Rust queries 快照重新排列了对象字段，审查确认全部 48 个解析对象逐字段相等；没有把字段重排后的快照误称为封存文件的原始字节副本。封存文件自身的字节摘要仍保持原值。

后续独立服务 [37773318110](https://github.com/jyqj/codecortex/actions/runs/37773318110) 的 attempt 1 实际成功，原 helper 退出码为 0，新增产品查询为 0。完整原件见 [实际回放档案](actual-raw-replay/37773318110/export-manifest.json)。原下载内容未变，`mode-restoration.json` 中实际 changes 为 0。323 个恢复副本文件、每臂 9 类派生文件和完整 1,812,623 字节扩展报告均与正式原件字节一致；扩展报告 SHA-256 为 `71123cbdf56f7f5b49af9be8d8f5ee67be228eee52f417e4e6ec1157c49ec9df`。

独立审查者于 2026-10-08 12:12:20 UTC 接受本次实际 raw-only 回放，见 [回放独立回执](workflow-control/reviews/actual-raw-only-replay-review.json)。回执为 13,705 字节，SHA-256 `efcb83e09a7ccb90827215a577e992dd2206e27144fce57ee669b0f700019986`。它核对了两份完整 ZIP、323 个恢复文件及 323 个 scorer 副本、18 个派生文件和 288 份原响应，并保留四个原 replay 命令的退出码 1 / 2 / 0 / 2。回放服务成功不改变原候选 gate 1。

本次回放运行在另外的 runner（AMD 9V74、ImageVersion `20261004.327.1`），环境按本次真实记录单列；不声称与正式执行使用同一硬件/镜像，也没有借回放重新测量产品延迟。

回放服务的 controller head 为 `8f5dcc1f0c4d9d2be1ebf2ade64397a0872a52cb`，只在原提交之后追加了独审文件和 replay-request。实际回放 checkout 原执行 utility `b5146043fd57e3edd2655397b5775fdc0f1d2e52`，使用原 scorer；只读取原来的 raw、normalized 和运行元数据。它验证的是原始字节绑定及原 scorer 的 9 份派生输出一致性，不称为独立 raw re-normalization。所有 ZIP 成员、日志、原 gate 失败、早期失败和 source-only 修订历史均保留。

## 原任务与发布边界

P8-004 的原步骤为“审查生产无 gold 路径/题词典”和“一次冻结配置运行 heldout”；原验收条款、依赖 P8-003、V19 入口和回滚要求原样保留。工程流程验收与质量 gate 判定分别记录。正式质量门失败仍为失败；未证实的 Recall@20、graph/facet、live semantic、100k、尾延迟、跨平台发行覆盖与旧 v1 restricted custody 保持未认证。

原候选 `2cd04485…` 的源码审查、原 CI、PR #153 的全部回归结果和一次保留失败记录的 CI 重试均可追溯。PR #156 后来的源码/600 DEV/CI 与本次冻结对象分别记录，不相互替代。下一原任务仍为 P8-005；P7-018 的真实 provider 认证继续 blocked。

## 主要入口

- [冻结测量协议](implementation/PROTOCOL.md)
- [固定协议 JSON](implementation/protocol.json)
- [封存数据](sealed-data/locks.json)
- [完整作者与独立审查历史](workflow-control/draft-history/reviewed-bundle-manifest.json)
- [公开 DEV 关系审查](workflow-control/leakage/actual-public-dev-review.json)
- [一次性授权原件](authorizations/37769636440/authorization.json)
- [原始执行结果](actual-execution/37769636440/execute/raw/execution/result.json)
- [固定报告与全部 288 行](actual-execution/37769636440/execute/raw/execution/replay/review.json)
- [实际回放完整档案](actual-raw-replay/37773318110/export-manifest.json)
- [回放独立回执及其实际附录](workflow-control/reviews/actual-raw-only-replay-review.json)
- [实际回放独立核验附录](workflow-control/reviews/actual-raw-only-replay-review-appendix.json)
- [P8-004 最终人工验收](workflow-control/reviews/P8_004_manual_acceptance.json)
- [本次证据索引](index.json)
