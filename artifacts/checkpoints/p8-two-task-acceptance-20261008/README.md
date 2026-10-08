# P8-002 与 P8-003 当前候选验收

本轮按原任务条款完成公开 native DEV 语料准入与固定外部套件执行/分报。当前候选是 `2cd04485b0e0b23483f64671d56538bbaa47f442`；所有构建、V02 和外测证据均保留自己的实际 source/run 身份。

## 任务与计数

本轮将 P8-002、P8-003 从 in_progress 置为 done。192 项原任务中 162 项完成、30 项未完成；相对本次用户请求开始时新增完成 9 项。原 acceptance、steps、depends_on、validations、rollback 等条款不变，PR152 已合并的证据和所有历史失败记录继续保留。P8-004 仍在进行。

## 公开 DEV 语料

六个固定公开仓库共 327 native、282 compat 问题，132 源文件、509 个 gold span、20 份唯一许可文件、305 个已知相关组件。182 个 canonical 文件逐字节、Git OID 与 mode 核验一致；当前 release evaluator 实际完成 16 个原 V02 检查、12 个新 freeze 和 12 个 suite validation，schema 无错误。真实运行：[37752879911](https://github.com/jyqj/codecortex/actions/runs/37752879911)。

327 低于初始约 600 的规划量；所有问题为英文 DEV，305 组件不代表已证明统计独立；14 个预留 Serde/Vite slots 未起草。这些差额和限制未被隐藏。完整人工验收见 `../p8-joint-public-validation-20261008/p8-002-independent-acceptance.json`。

## 固定外部套件

原 cc-switch/Flask 输入锁、同一实际 release package、top10/repetitions3/warmup0/timeout30000/seed20261003 与原 600s 命令预算保持一致。四组各 100 个 ID × 3 次，共 1200 条结果及 4104 个命中，分别保留 normalized/scores/metrics/gate/manifest 和原 replay。真实运行：[37749276827](https://github.com/jyqj/codecortex/actions/runs/37749276827)。

| 数据集 / profile | 原状态 | Top1 | nDCG@10 | 原 gate / replay |
|---|---|---:|---:|---:|
| cc-switch / compat | 300 Partial | 0.26 | 0.2556726945 | 1 / 1 |
| cc-switch / native | 300 Partial | 0.12 | 0.2517602016 | 1 / 1 |
| Flask / compat | 270 Partial、30 NoMatch | 0.49 | 0.4997546424 | 1 / 1 |
| Flask / native | 270 Partial、30 NoMatch | 0.44 | 0.4978057483 | 1 / 1 |

四组质量门均失败。原 driver 的自动未结项标志和 `release_certified=false` 保留原字节；单独人工验收只判断原 P8-003 的运行与分报条款，不将 exit1 改成通过，也不声称外部 native 机械 gold 已经语义认证或可以直接比较排行榜。完整验收见 `../p8-joint-public-validation-20261008/p8-003-independent-acceptance.json`。

原证据位于 `../p8-release-evidence-20261008/runs/37749276827/`，包括完整 Actions ZIP 的 38 个无损分片、原报告和独立核验。旧 `cc514` 的 run37746625347 独立保留，未重标为当前候选。

## 回归与后续

当前候选的 CI、engineering、closeout 三组检查均实际通过：37749230722、37749230693、37749230656。174 个 source-integrity 测试和 schema9/1038 输入检查保持原断言；release 实际执行 39 wrapper、4 profile、5 scanner、2 watcher、5 readiness unit 与 2 readiness real 测试。

P8-004 的真实前瞻留出数据尚未起草或执行，预检与准备工作不计为任务完成。G8、完整发布质量、100k 规模和受保护旧 holdout 的状态保持独立。
