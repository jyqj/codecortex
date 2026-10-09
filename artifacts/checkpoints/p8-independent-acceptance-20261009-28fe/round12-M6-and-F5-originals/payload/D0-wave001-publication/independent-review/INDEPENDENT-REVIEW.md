# D0 recovery wave-001：非作者独立审查

## 结论与审查范围

**冻结版满足 wave-001 的启动控制要求。完整 150 格 continuation coverage 尚未验收。** 本结论仅覆盖 `(100000, repetition 8, supplemental ordinal 1)`，以前驱原 run `37854240827` / attempt `1` / job `113580044384` 为唯一准入对象。

Root 可继续既定发布步骤：核对实际新 commit、G4 父提交、12 项 tree/blob 与注册字节；发布前再读当前 rep8 的官方 job、完整 log 和原 run artifacts。运行时仍须通过既有 admit、execute 前刷新和 job 末 recheck。此审查的正向 API fixture 是 **2026-10-08 23:55:57 UTC 的冻结历史**，不能当成当前状态或发布前刷新。

`coverage_accepted` 始终为 false。旧 ca72 study 保留其原注册下 failed/incomplete；本审查关闭原 TODO **0 项**，不授予完整 continuation、G8 或 release 通过。

## 冻结身份

| 对象 | SHA-256 |
|---|---|
| `recovery.py` | `2a6468f4e3d8cf8a24e22b7a4517ab197be34018148f0f25cf241f8431806c54` |
| `registration.json` | `d968cb6aaa680b877fea26e6270bb9a651d7262958f6730fa97b13d71c59dec1` |
| `.github/workflows/p8-d0-recovery.yml` | `486d726c9c573a4e5e9b8be4383696d9cf5983fad4431b44370e115ffe8d9c4f` |
| 作者 `candidate-receipt.json` | `7f62649bdafdc196268358e6335b417228f4f534c7607f457e3596eb3f31f0bf` |

独立复制至 `recovery-audit/snapshot-final/candidate/` 后核对全部 **12 文件、614793 字节**：文件集合、每项字节数、SHA-256 和 Git blob SHA-1 均与候选收据相符。被测源固定 D0 `d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`；控制器拟基于 G4 `260f596582f2d82b8d7c707b61a6b8b6a43b069f`，真正新控制器 commit 尚由发布步骤登记。

## 原条款的独立核对

直接核读原 `09-BENCHMARK.md` 第 136、173、183、185 行、`06-VALIDATION.md` 硬正确性条款，以及 D0 `P8-SCALE.md` 的不可覆盖、完整样本、预算、原 raw 重放与 host 分层规则。原规则要求保留失败、partial、timeout/censored 与全部样本，禁止 best-of；未发现项目级“历史发生一次 infrastructure failure 后永远不得恢复”的条款。

原 ca72 注册另有 `Retain every failed/missing cell; do not rerun, replace or substitute cells`、`workflow_attempt: 1`、全部 150 primary 为新测量的约定。原 workflow 三处 attempt1 守卫及 `admit.py` 的 attempt1 守卫确实存在。因此本新范围必须称为“固定全部初始 attempts 的事后采用 + 新 supplemental attempts 的前瞻登记”，不能据此重判原 study passed。

原 rep8 保留 exit143、shutdown 原日志、upload skipped、root cause unknown、native outcome unknown。观察到的 job/step 时段不能代替有效 native latency，也不能把未上传内容填为 native 0 失败、0 操作或 0 耗时。

条款来源为 `acceptance-review/round10-scale-infrastructure-recovery/sources/` 的原件及 `runtime-review/round9-D0-study-readback/` 的原注册、workflow、admit；提案 v2 SHA 为 `895c34dadac30a06357ac10036c04a3dad64b48d2e4684a7bf021257cad088e1`，已核原字节。

## 发现与修复

初审控制器 `8729aa4279cfd50386d5535fa619140ba15c6e5c6020a995477ba33be32d3ebe` 的 22 个独立离线探针中有四个反例未被拒绝；首次脚本、快照和结果全部保留，未覆盖。

| 初审问题 | 冻结版修复及复核结果 |
|---|---|
| 原 run API 对象的错误 `id` 仍被准入 | 核固定 run id、controller、attempt、event、branch、workflow path 与 repo；反例现被拒绝。 |
| 未匹配名称的额外 job 被忽略 | 完整 150 measurement、固定 admission job、唯一可选 aggregate 之外的 job 拒绝，并统一核 run/source 与全局 job id。 |
| 未分类额外 artifact 被忽略 | 核允许名称、唯一 id/name、原 run/controller、digest/size/expiry；真实原 9 个 artifact 正例通过，未知对象、重复、来源漂移均拒绝。 |
| 相同 job id 换 run 元组可重复记账 | 改为全局 job id 唯一性；原反例被拒绝。 |

同时核实：非目标官方失败保留 `unclassified_official_failure`、native unknown 和未读 log 状态；启动收据仅记 driver launch 请求，native started 保持 null，完整原 raw validator 通过后才记 true。控制器 JSON 拒重复 key 和非有限数；输入采用固定纯文件名集合与摘要、regular-file 检查。原 build/shard validator 的完整 inventory、symlink 拒绝、source/driver/binary 与原计划校验保持调用。

固定 workflow 只有一个明确 cell，无 150 格新矩阵。自身 attempt1、固定并发组、当前专用 branch/workflow 完整 run 历史唯一性共同阻止该 wave 的重复启动。初始 150 identity 全部冻结；首 wave 只接受 prior supplement 初值 0。此范围不声称已经发现整个仓库所有任意未登记实验，后续发现额外 lineage attempt 会使当前依据失效。

每次 observe 保存原 API/log，再核完整分页、前驱终态、固定 log 字节及新出现的原 rep8 raw。known native/parity/source/evidence/deadline/预算错误、active/queued、已有效成功和未分类前驱均不准补过。recheck 失败追加撤销收据并非零退出。

## 独立验证

- `probe_final.py`：**34/34 按预期**，含真实冻结 150 identity / 151 job / 9 artifact / rep8 完整日志正例、四个原缺口的回归反例、缺格、pending、已成功、known failure、迟到 log/raw、重复 wave、旧 rerun、来源漂移、错误 branch/workflow、非有限 JSON，以及非目标失败仍保留 unknown 的正例。
- 冻结候选随附 `test_recovery.py`：在审查员自己的目录重新执行，**33/33 通过**。原输出见 `author-controls-replayed.log`。
- 本审查新启动 native measurement、GitHub run、重试、取消、远端修改均为 **0**。上述计数只表示控制器审查，不计作原 TODO 完成或原 1500 样本。

## 后续完整验收仍须实现

后续 receiver 必须收齐实际 run/job/attempt/artifact/ZIP 身份、所有 planned/dispatched/started/terminal attempts、遗漏、失败及 unknown，跨 wave 累账且不因改名重置；只有允许中断链才能采用 supplemental，原有效完整格必须采用原件。它还须保留并消费 job 末 recheck，以及随后到达的阻塞证据与撤销记录。

只有完整 150 唯一 coverage 身份、1500 原 group samples、所有原 raw validator/combine 及外层账本同时通过后，才能形成新的完整 coverage 结论。host 分层、实际每层 N、全部 attempt 分母与未知耗时必须透明；既有单格成功不会抵消已知原产品失败。原任务硬依赖和最终源码适用性继续逐项审核。

这些是本次授权范围中尚未验收的功能，不是新增项目永久阈值。当前 wave-001 的 12 个冻结文件不因后续 receiver 工作而修改。
