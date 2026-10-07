# P8 后续十项：实现、证据与 PR 管理

本批按原始任务 ID 推进 `P8-002/003/004/011/012/014/015/016/017/020`，三轮已交付 **4 + 4 + 2 = 10 项，本批剩余 0 项**。任务定义、验收条件及硬依赖不变。工程推进不等于完整任务验收；逐轮计数以 [进度表](../../../docs/roadmap/code-index-v2/P8-NEXT-PROGRESS.md) 和 `tasks.json` 为准。

| 轮次 | 原任务 | 累计推进 | 本批剩余 | 全库尚未完整验收 |
|---|---|---:|---:|---:|
| 1 | P8-002、004、012、016 | 4 | 6 | 40 |
| 2 | P8-003、011、014、015 | 8 | 2 | 40 |
| 3 | P8-017、020 | 10 | 0 | 40 |

第三轮结束时任务状态为 **192 = 152 done + 22 in_progress + 17 todo + 1 blocked**。本批十项全部保留 `in_progress`；当时主线未验收数从 42 降至 40，来自 #144 的两项正式关闭，不将本批有限工程交付冒充十项完整验收。

后续合流采用 #147 固定 `434145f6…` 的 P7-013 工程验收与 P7-020 追加证据，当前为 **192 = 153 done + 21 in_progress + 17 todo + 1 blocked，39 项未验收**，下一项 `P7-014`。[独立审查](release/pr147-doc-progress-review.json) 接受原条件下的 P7-013 工程范围；三轮 P8 历史计数和全部十项状态保持不变。旧 G8 运行仍绑定原任务快照，P7-020/G7、真实 provider、G8 和发行没有获准。此次合流只增加证据与进度，785 个产品输入、v12 及当前 CI 字节不变；新 head 的 CI 独立记录。

## 第一轮

- `P8-002`：固定公开 DEV admission 的 Git 对象、查询身份、native/compat 投影、源文件与 gold span 审计；实际 183 个输入、301 native / 256 compat。正式多语言 600 条与独立 holdout 认证仍开放。
- `P8-004`：开发语料边界与生产路径签名扫描、原始发现及人工定位。保留 2 条原始 `review_required`，独立 holdout 正文读取为 0；不据此声明语义层面的无过拟合。
- `P8-012`：Linux/macOS × MSRV/stable × default/semantic 八格冷构建编排，编译器、输入、产物与失败证据绑定。第一份矩阵八格均 `not_run`；第三轮另补一格真实冷构建，见下文，旧矩阵不覆盖。
- `P8-016`：既有固定 default/semantic 产物的有限离线回滚，四次真实 stdio 启动、未来 schema 注入和受控重建、恢复备份、配置切换。旧产物来源按原收据保留；未知 cache 格式与真实相邻版本包回滚未执行。

[语料/边界证据](corpus/) · [平台/回滚证据](platform/) · [本次源码独立审查](source-review.json)。

## 第二轮

- `P8-003`：固定公开 Express DEV 的 native / compat 两轮比较，实际发出 774 个请求，输入校验、run 和原始重放退出 0。原 compare 为 exit 1：compat 因 n=177 与硬件身份不足为 `inconclusive`，native 的 p95 比值 1.2127714 超过原 1.2 门槛且硬件身份未知。后端是固定 rg / snapshot control，不能替代完整产品兼容性验收；外部 cc-switch / Flask 原基线仍 `not_run`。首次错误 CLI profile 的退出 2 原记录保留。
- `P8-011`：当前固定候选的五次真实 stdio 启动完成三项有限故障恢复，保留 kill/restart、真实数据库 busy、warm cache 删除后的原始观察。早期 source-manifest 误读 DB/WAL/SHM 影响锁观察的问题已修复，首次 2 fail / 1 pass 和后续真实控制均保留；内部事务崩溃、真实 provider 网络、活动 semantic cache 和并发数据库切换四项未执行。
- `P8-014`：judge 输入盲化、输出对账和人工复核队列，18 项控制通过；没有真实模型调用，gold 不变，不能声称 LLM 复核完成。
- `P8-015`：semantic 认证材料预检，24 项控制和三次实际 CLI 场景。缺材料、完整纸面材料、fake fixture 均不能代替真实 provider / custody / approval / scoring 认证，发布保持 blocked，没有模型费用。

[兼容性证据](compat/) · [恢复证据](recovery/) · [judge / semantic 证据](release/)。

## 第三轮

- `P8-017`：消除 `benchmark::statistics` 的 `distribution` / `quantile_interval` 两条路径重复 nearest-rank 计算，12 项实际测量回归通过，并完成固定源码的独立审查。整个 crate 的其他百分位实现、临时旧 branch、重复 schema 和外部 wire 兼容清理尚未完成。
- `P8-020`：新增原 G8 范围的依赖和证据缺口总账，保持 192 个原任务定义及 G8 原文。local 有 17 个直接前置、semantic 有 18 个；optional judge / P9 不追加为硬阻断。19 项控制通过。实际固定 13 条仓库证据及空证据负控均退出 1、local / semantic 均 blocked；所有任务和纸面记录声明通过也不能产生发布批准。历史 f99 任务快照和最终任务状态使用不同运行，不能覆盖旧回执。

另为 `P8-012` 补充 **Linux / Rust 1.95 / default / dev** 全新 target 的一次真实冷构建，151.146 秒、构建退出 0、严格 `--verify` 退出 0，实际 features 为 `[]`。完整 785 输入与固定 `78ae91ee…` 候选相同；矩阵为 **1 passed / 7 not_run**，矩阵总进程退出 2 表示未完成，不能声称 MSRV、macOS 或 semantic 冷矩阵通过。此项不重复计入十个 TODO。

[限定清理审查](cleanup/) · [G8 总账](release-review/) · [实际冷构建](platform-cold-default/)。

## 源码与验证范围

本地固定 Rust 源码 `3f6cca74c1c9da16cd5eca818c1bce57659d1db0` 与已发布 source commit `78ae91eeae6edae6bea29c27f24b251773341c00` 的全部 **785 项 crate/Cargo 输入**相同。独立审查记录固定在 `7f992a0e328aa1f088746dca360d7d5302e2f2e7`，涵盖 16 个相对 P8 候选变动的路径。

整合保留 #144 的 dense coverage、GC、worker 与 strategy 变更，保留 #145 的 P8 工具及证据。额外修复稳定 Rust 的 deprecated atomic API，采用保持限额和溢出语义的 CAS 循环；`benchmark::statistics` 中 `distribution` 与 `quantile_interval` 两条路径共享 nearest-rank 算法，并以独立整数 oracle 验证临界样本数与重复值。均值 `bootstrap` 的原区间算法不变；`bench.rs`、`report.rs` 的其他百分位实现仍待后续清理，不能声称整个 cc-eval 已统一。

v10 入口分别重建两套历史接受链，再应用固定审查的顺序差异；两套旧 guard/registry 字节、历史拒绝条件及 CI 原有步骤继续核验。随后为修复 worker TEST 就绪顺序，固定新源码 `65dd32934b3f8cb3ff4f5f5deb154a431a8c09e3`：仅 `crates/cc-eval/tests/p7_worker_contention.rs` 一个输入变化，其余 784 项保持不变，产品库代码不变。独立审查固定于 `b64746f42750422dc38aa9bb73a6f2f2b43fdc2b`。

固定 v11 先精确验证固定 48efa 快照中的 v10 脚本和 registry，再完整执行 v10 旧证明，最后只接受新审查授权的单个 TEST 差异。历史 selector 测试使用原始固定 workflow blob，在读取前显式确保完整 commit 引用已获取；原负控、拒绝条件和 CI 步骤均保留。源码准入只证明来源和字节，不继承完整质量、100k 或发布结论。旧 P8 产品执行记录继续绑定 `78ae91ee…`，不重标为 `65dd3293…` 上的执行。

#145 随后并行合入 main `d53a4972…`。合流候选固定为 `8e12c3884edbb2743eb2aee82fafa285e8b28ef7`，独立审查固定在 `3056a14ccc3496b4e5c9ff1e3746bf6cf33a1b95` 的 [双来源审查](release/main-merge-source-review.json)。相对 v11 的 785 项输入，仅 `p8_load.rs` 的 `cfg(test)` 模块新增 43 行，生产部分原字节保持；原有两项预算测试和 main 新增两项测试全部保留。相对 main joint22 产品的四处差异逐项列出 before/after 摘要，其余输入保持一致。

合流版本选择 v12：先完整执行 v11 链，再执行 main 原 joint22 guard 的完整证明和原 CI 核验，最后按固定双来源审查接受一处新增差异及四处来源差异。旧脚本、registry、main 原测试和 workflow 原字节另存固定快照，当前 CI 仍保留全部 `test_p8_*.py`、历史核验及私有 default 产物路径。原证明和新证明都实际执行；旧运行不重标成合流候选的结果。

固定 delivery `0e197ff3…` 已通过 [最后独立审查](release/main-merge-final-review.json)：main 的 11,162 个 artifact 路径、模式、blob 全部相同，104 个既有测试方法和原断言保留。完整 v12 CLI 实跑 209.670 秒、退出 0，18 个新增控制和 4 个历史方法全部通过；[原始结果与中断记录](ci/main-merge-validation/) 分别归档，不将中断尝试改记成功。该实际执行与审查时的完整 Git 身份保存在 [补充历史](history-main-merge/)，后续新增归档不重标原执行 commit。

| 实际执行 | 结果 | 原始证据 |
|---|---|---|
| 最终 Rust fmt | exit 0 | [fmt-final.log](validation/fmt-final.log)，含 worker TEST 顺序修复 |
| Atomic 限额与并发测试 | 2 passed / 0 failed | [atomic-budget.log](validation/atomic-budget.log) |
| P8 CLI/load/measurements/route/scale 回归 | 34 passed / 0 failed / 0 ignored | [p8-regression.log](validation/p8-regression.log) |
| 全 workspace、all-targets 严格 Clippy | exit 0 | [clippy.log](validation/clippy.log) |
| 第一轮全部 P8 Python 控制 | 100 passed，包括 61 个本轮新控制 | [round1-python.log](validation/round1-python.log) |
| 三轮集成后的全部 P8 Python 控制 | 197 passed / 0 failed，18.158 秒 | [p8-final-python-tests.log](validation/p8-final-python-tests.log) |
| v10 新源码与 CI 拒绝控制 | 12 passed / 0 failed | [source-v10-tests.log](validation/source-v10-tests.log) |
| 实际 v10 源码准入 | passed，785 inputs | [source-v10-cli.json](validation/source-v10-cli.json) |
| 实际新版历史与任务定义核验 | passed，192 原定义、4 个视图 | [historical-v2.json](validation/historical-v2.json) |
| 最终任务快照历史与定义核验 | passed，192 原定义、4 个视图；tasks 摘要 d69e664a… | [historical-v2-final.json](validation/historical-v2-final.json) |
| v11 新源码拒绝控制 / 实际准入 | 10 passed，70.691 秒 / exit 0、785 inputs | [测试](validation/source-v11-tests.log)、[准入](validation/source-v11-cli.json) |
| 修复后原 worker semantic 用例 | 1 passed / 0 failed / 0 ignored，9.62 秒；384 条请求 | [真实执行与原始材料](worker-ready-fix/) |
| 合流后的四项原预算测试 | 4 passed / 0 failed / 0 ignored / 46 filtered；warm 依赖，19.82 秒总耗时 | [完整原始记录](ci/main-merge-budget/)，实际本地 source 为 b899，785 输入与 8e12 相同；保留 Cargo 磁盘 metadata warning |
| 合流前固定 2a75 的 P7 engineering CI | 24 组 Rust：95 passed / 0 failed / 1 ignored；15 项 Python；worker 384 条请求完整 | [原始日志与官方 artifact 检查](ci/pre-main-p7/)，不覆盖随后合入的 main |
| 本地全 workspace Rust 测试 | 编译阶段因临时空间上限主动停止，exit 130；测试未执行 | [原日志](validation/workspace-tests.log)、[停止回执](validation/workspace-test-stop.json) |

首次本地完整 source suite 实际运行完毕，为 **94 tests：92 passed / 2 failed**，1059.377 秒；失败是旧 selector 假设。其后两个修复方法实际通过，完整原始失败保留在 [回执](validation/source-tests-first-result.json) 及 [日志](validation/all-source-tests-first.log)。没有宣称在本地重跑整个修复后的历史 suite；最终远端完整 CI 负责验证全部旧链和浅 checkout。

新评测器和服务器真实构建的 [cc-eval witness](validation/cc-eval-build-witness.json)、[codecortex witness](validation/codecortex-build-witness.json) 记录实际命令、二进制摘要、日志和完整输入清单。它们是构建后的源码/产物对应记录，**不是新冷构建收据或编译器密码学证明**。其中服务器命令复用前述测试 target 的依赖，未加 `--no-default-features`；实际 Cargo artifact 的 features 为 `[]`，原始 JSON 保留。

执行所引用的原始本地 Git 身份保存在 [增量历史快照](history/)。新 clone 可恢复 `f99b2dc2…`、`2d060834…`、`83dae5a6…` 等原始 commit 后重放固定回执，不依赖此次临时工作区，也不把同 tree 的远端发布 commit 当作原执行 commit。

独立新 bare 仓库已实际验证这一恢复流程：两个 depth=1 远端前置之后三个本地对象仍不存在，导入后全部可读；重新运行一次 G8，原 stdout / receipt 各自逐字节复现，任务计数及 blocked 结论不变。[恢复审查](history-restore-review/verification.json) 保留 16 条实际命令和完整身份对照。

## PR 管理

#144 的固定 head `eb7cdc55aa94c8d6865bed14fa37fff08080af33` 已确认两套 GitHub workflow 成功：
[CI 37657882021](https://github.com/jyqj/codecortex/actions/runs/37657882021)、[P7 engineering 37657882000](https://github.com/jyqj/codecortex/actions/runs/37657882000)。以 expected-head 保护合并成功，main merge 为 `b951f27d3ed50b7755bc2456c6425355f753ec17`，主线剩余 40 项未验收任务。

#145 的原失败保留：stable Clippy 拒绝 deprecated `fetch_update`。本批已修复且严格 Clippy 通过；原失败没有改标成功。

此前 PR 复查发现 #145 已独立更新至 `ebf645e9821e57b78d5dfee267e720f7312731a8`。独立审计固定本分支 `4ff6df76…` 对照：相对 #145 首版的 372 个路径，其中 325 个相同、47 个不同，含独立新增证据和不同源码准入链；两边完整 crate/Cargo 清单有四文件差异。当时保留该 PR 开放，继续分别判断两套 CI，没有将算法近似当作采用整个新 PR。

2026-10-07 18:37:23 UTC，#145 被并行合入 main `d53a4972af92fd10a5cddb9f15ffdf06414b3d54`。随后本分支保留它相对旧 main 的全部 **214 个 artifact 路径**，每个 Git blob 完全相同；保留 P8-LOCAL-PROGRESS 追加记录和 P8-001 的最新完整对象，再生成四个任务视图。十项本批推进及 **152/22/17/1、40 项未验收** 不变。原先 2a75 的检查继续按固定旧版本归档，合流候选另跑 CI。

[集成 PR #146](https://github.com/jyqj/codecortex/pull/146) 第一份 head `48efa5a6…` 的两套 CI 原始失败也完整保留：[main CI](https://github.com/jyqj/codecortex/actions/runs/37662135799) 通过 fmt、Clippy、默认编译和默认回归后，source suite 出现 2 fail / 1 error（旧 selector 假设及浅 checkout 未取到固定 ae906 fixture）；[P7 engineering](https://github.com/jyqj/codecortex/actions/runs/37662135135) 在 worker 就绪观察时遇到 `RetrievalChanged`，发生在测量开始前。分别修复固定 ref 获取、selector 迁移和就绪检查顺序，没有降低断言或扩大预算。

该 main CI 的默认 Rust 结果共 190 组、**2612 passed / 0 failed / 65 ignored**；忽略项保持原状态，失败步骤之后的 HTTP/MCP/eval 步骤被跳过，不能算通过。[main 结果及完整压缩日志](ci/first-main-result.json)、[P7 结果及完整压缩日志](ci/first-p7-result.json) 保留实际来源与摘要。新 head 的最终完整 CI 以 PR 当前检查为准；本归档封存时不提前宣称成功。

随后固定 `2a75e65d…` 的 [完整主 CI 37667560396](https://github.com/jyqj/codecortex/actions/runs/37667560396) 已实际成功，check 的 35 个步骤以及 MSRV、security 全部通过。[正式日志与逐组解析](ci/old-head-2a75-full-ci/) 保留全部原件：Rust 执行累计 3115 passed / 0 failed / 126 ignored，5 组 Python 共 370 项均通过。该累计没有对重复执行去重，392 个 Rust result 组中 151 组没有实际执行测试，不能把空组称为覆盖；ignored 的原名称和原因逐项保留。这只覆盖当时旧 main `b951f27d…` 上的 `2a75e65d…`，合入 `d53a4972…` 后的结果单独按新 head 判断。

其余 43 个旧草稿仍有独立改动，不批量关闭或删除分支。它们的存在不影响本批按原始任务 ID 去重计数。最终 PR 状态与 CI 以实际 GitHub 结果另行追加。
