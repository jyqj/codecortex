# 最终 CI、合并回执与 39 项接续清单

[PR #146](https://github.com/jyqj/codecortex/pull/146) 已按最终 head `9b3711a9346b64868b64cf6cc1f6e38162a59a43` 合并到 main `47d1939e43455ecd72ccc73e80825d971e3f33e7`。实际合并完整 tree 为 `5fd3e14bfabd98346a4fb0b18c28b04c8b212bec`，与最终 PR head 和实际 CI checkout `0b69df3c…` 的完整内容相同。

当前 **192 = 153 done + 21 in_progress + 17 todo + 1 blocked，39 项未验收**；下一项 **P7-014**。三轮推进十个原始 P8 TODO，批内剩余为 **6 → 2 → 0**，当时全库未验收数均为 40。随后采用 P7-013 的独立工程验收，当前减少为 39；这次结项不增加第十一项 P8 推进。完整机器记录见 [delivery-receipt.json](delivery-receipt.json)。

## 最终版本的正式 CI

| 官方执行 | 正式结果 | 完整证据 |
|---|---|---|
| [主 CI 37674979736](https://github.com/jyqj/codecortex/actions/runs/37674979736) | check 35 步、MSRV 9 步、安全 5 步全部成功；5 组 Python 共 388 项通过；392 组 Rust result 累计 3119 passed / 0 failed / 126 ignored / 1855 filtered out | [主 CI 归档](main/ci-evidence.tar.gz)、[逐项索引](main/verification-index.json)、[文件摘要](main/file-manifest.json) |
| [P7 engineering 37674979650](https://github.com/jyqj/codecortex/actions/runs/37674979650) | 14 步全部成功；24 组 Rust 为 95 passed / 0 failed / 1 ignored，15 项 Python 通过；384 次请求和 785 项源码输入全量核对 | [P7 原件与复验脚本](p7/p7-final-evidence.tar.gz)、[产物检查](p7/artifact-check.json)、[文件摘要](p7/file-manifest.json) |

以上是现有命令的实际执行累计，包含重复执行的 suite。主 CI 有 151 个 Rust result 组没有实际执行测试；所有 ignored 的原名、原因与行号保留。P7 唯一 ignored 的 stdio mechanism smoke 需要显式当前产品路径和 SHA，保持未执行。既定 CI 的范围、原筛选条件和未验收的完整产品门保持各自原定义。

P7 的正式 ZIP 有 3 个 seed × quiet/held × C1/C4 共 12 格，每格 32 个 ordinal 恰好一次，384 条请求均命中 stable.rs；旧 held 输入发布数为 0。原 2s/5s watchdog 保持不变，完整 P7-015 仍未验收。source-before/after 原字节一致，785 个输入逐项对应 `8e12c388…` 与最终 head，canonical map 为 `4c9aeb9cac2eea382d2dbce6d85220af3e10f6483b4c1f696b7874ecaff0fdb4`。

主 CI 日志也实际输出新 39 项状态、tasks SHA `a7ebacf3903f3f405e3c5ba4d19f5590107467b55ede35c4d675516c07cf4e68`，v12/785 输入通过。两名独立 subagent 取得的主 job decoded 日志逐字节相同：1,001,039 bytes，SHA `bb8043f85357c2d813b21ae39773cdfbdce6a36c1b481badf1e653a86a842433`。

## 接续与历史记录

[39 项接续清单](continuation-39.json) 固定原任务条件，列出 P7 7 项、P8 20 项、P9 12 项及本批十项尚缺的验收。P7-014 的原前置 P7-013 已完成；新字段的 schema/sanitize/handler/doc/E2E、原 mode 语义，以及未配置/关闭/回填/失败/就绪五态一致性仍按原要求落实。条件性 GC count/log cap 与其他任务的责任不升级为新硬依赖。历史 5af7ac 收据保留其来源标签，本次未能取得该 Git 对象，不将它当作新 head 的执行。

前一 head `57cec179…` 的 [完整主 CI 原始归档](prior-57/main/ci-evidence.tar.gz) 单独保留，其 40 项状态不改写为 39。更早 CI 的原始失败、中断、本地历史 bundle、预算测试、v12 验证和三轮交付仍在上级证据目录。当前正式验收范围没有扩大到 G7/G8、真实 provider、正式 holdout 或完整跨平台矩阵。

[合并对象原件](official-merged-commit.json)、[最终 workflow 状态](official-final-workflows.json)、[49 步原状态](official-final-main-jobs.json) 和 [43 个历史草稿快照](remaining-prs.json) 固定交付时状态。补充证据通过本会话既有证据分支保存，保留实际测试 merge commit 的父引用；已合并的产品与任务树保持原字节。

每个压缩包都含可携带的只读复验脚本和文件 manifest。P7 解包后执行 `python3 verify_artifact.py --repo /path/to/codecortex`；主 CI 解包并还原日志后执行 `python3 parse_log.py <log路径> <新输出JSON路径>`。这些命令核对原始材料，不运行新的产品测试。全部交付文件摘要见 [file-manifest.json](file-manifest.json)。
