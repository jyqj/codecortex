# 第 30 轮验收记录

本轮原始 TODO 新增完全完成 **0**；192 项中已完成 **163**，剩余 **29**。目标仍为 P8-005 至 P8-013 以及 P8-016 共十项。源码审查、测试数量、工件和清理记录不计作新增完成项。

本记录固定 **2026-10-09 11:40:47 UTC** 截止时的事实。容器在截止时间之后制作，内容选择仍属于这一轮；后续运行和审查另行追加，不回写这里的历史状态。

| 范围 | 截止时已有证据 | 截止时仍待完成 |
| --- | --- | --- |
| 原 C 规模研究 | 原 helper 已接受 4/150 分片、41/1500 样本 | 100k preflight 与原自动后续矩阵；完整研究尚未完成 |
| 原 C runtime | 四档 mixed 的原件各自复核通过，加上上一轮 backfill，累计 5/6 组 | 原一小时 soak 尚未完成独立原件验收；DB acquisition 补充属于独立 LG 来源 |
| 原 C 平台 | 新增 Linux 1.95/semantic 与 Linux 1.95/default 的原件验收，累计 7/8 格 | 最后一格的原件验收及完整八格 collector |
| LG 诊断 | 上一轮原 v15 实际成功，固定实现与原件验收器已审查 | 截止时 admission/control job 仍 queued，尚无本次 Rust 或 feature-on mixed 结果 |
| PR #180 与主线关系 | 当前 PR 来源仍为 1d39；对新 main b941 与 M2 的来源差异和保留方案已有只读记录 | M3 的实际源码准入不在本轮结果内，未来实际整合须另行固定来源 |

原 C 固定为 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`，LG 固定为 `18255c53b7fa153bb71c96f57f1b97ca139e427f`；截止时观察的 PR #180 为 `1d39f382d4e08e702a55693c9c614fa477bf7e0c`，main 为 `b9412406e11422d7cf914458a8bfbbd58cf94eaa`。各来源分别保留，不把其他来源或后续运行改记为原 C 的测量。

本轮仅为平台验收脚本增加可选 artifact-root 路径参数，默认路径、27 个原谓词、来源校验、八格集合及外层 250 MiB reserve 均保持。该副本经过独立审查后用于私有 RAM 中的原件复核。原 helper 保持原字节。旧 173 派生文件、实际 Git 临时缓存及 runtime 派生空间的受限清理保留逐字核验和失败边界；原 ZIP、验收报告、未知或 mode 不符的路径没有因此被当作可丢弃原件。

## 交付与复核

- [round30-closeout.json](round30-closeout.json) 固定本轮计数、截止状态与剩余条件。
- [round30-public-evidence.tar.gz](round30-public-evidence.tar.gz) 含 **140 个 regular-file 成员、4,099,174 bytes payload**，完整保留本轮选择的原日志、审查和辅助记录。
- [round30-public-evidence-inventory.json](round30-public-evidence-inventory.json) 列出每个成员的源路径、目标路径、bytes、SHA-256、Git blob 与 mode。
- [round30-public-evidence-readback.json](round30-public-evidence-readback.json) 保留全部成员的完整读取验证。
- [round30-public-bundle-inputs.json](round30-public-bundle-inputs.json) 与 [build_public_round30_bundle.py](build_public_round30_bundle.py) 固定选择与确定性容器构建方式。
- `details/` 提供本轮主要 mixed、平台与来源边界审查的直接可读副本；其字节与容器成员相同。

容器大小 **1,990,989 bytes**，SHA-256 `ef44ae3b35cd5633ad0beae4475cb8b01ecd38523415432c7052c19b563a501f`，Git blob `ba11acc0bf9a7a0d210c2f266799989b50bb04b2`。使用标准 gzip/tar 工具解包后按 inventory 校验即可恢复所选记录。完整原 GitHub ZIP 另行保留；此包不宣称包含这些 ZIP 或全部原始二进制。私有传输 capture、file identifiers 和签名 URL 不在公开选择中。

本提交只新增证据路径，原任务定义、依赖、状态和已有证据不变。第 31 轮的 soak、LG 实际控制失败及其修复、最后平台格和 collector、M3 实际准入均不被追记到本轮。

## 官方来源

- [原 C 规模研究](https://github.com/jyqj/codecortex/actions/runs/37910924354)
- [原 C runtime](https://github.com/jyqj/codecortex/actions/runs/37908825814)
- [原 C 平台与恢复](https://github.com/jyqj/codecortex/actions/runs/37908825722)
- [LG 锁等待诊断](https://github.com/jyqj/codecortex/actions/runs/37919399759)
- [PR #180](https://github.com/jyqj/codecortex/pull/180)
- [上一轮固定证据提交](https://github.com/jyqj/codecortex/commit/95c7144abe8d8d0c4fe94e126727863319ffd579)
