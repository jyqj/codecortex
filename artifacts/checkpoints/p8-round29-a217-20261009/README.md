# 第 29 轮验收记录

本轮新增完全完成的原始 TODO：**0**；192 项中已完成 **163**，剩余 **29**。目标仍是将 P8-005 至 P8-013，以及 P8-016 共十项推进到原定义下的完全完成。测试、证据条目和源码审查不计作新增 TODO。

本记录固定第 29 轮截至 **2026-10-09 11:08:55 UTC** 的结果。第 30 轮及其后的运行状态另行追加。

## 本轮实际验收

| 范围 | 已验收结果 | 仍需满足 |
| --- | --- | --- |
| 原 C 规模测试 | 50k 新分片通过原 b107 helper；累计 4/150 分片、41/1500 样本 | 原 100k preflight 完成后，原工作流自动展开 145 个测量任务；完整研究尚未完成 |
| 原 C 回填 | 原件 11610619483：3 seeds × 2 conditions × 4 concurrency levels × 32 = 768 requests；原 helper 通过 | 此处的可用性探针不能替代混合负载的实际锁等待测量 |
| 原 C 恢复与回滚 | 原件 11611995300：499 ZIP members / 495 sealed members；原 rollback 与 011 验收均通过；25→24→25 | 保留原 not_run 限制；不产生 live、质量或发布批准 |
| 原 C 平台 | 新增 Linux stable/semantic 原件验收；累计 5/8 环境 | 剩余 3 个 Linux 环境及完整 collector |
| 原 C P7 closeout | 最后一项 default 检查通过，原四项全部成功 | 不转化为规模、live quality 或发布验收 |
| LG 锁观测 | 原 v15 源码守卫实际执行 exit 0；原件验收 helper 已独立审查 | 运行 37919399759 仍排队，没有实际 Rust/native 结果 |
| 当前 PR #180 | 保留外部 1d39 主干整合、原 PR 正文以及追加的第 28 轮证据说明 | 新版本独立 CI 尚无实际结果；未来 M2 + LP 只完成只读设计 |

原 C 源码固定为 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`；LG 诊断源码固定为 `18255c53b7fa153bb71c96f57f1b97ca139e427f`；本轮观察到的 PR #180 源码为 `1d39f382d4e08e702a55693c9c614fa477bf7e0c`，main 为 `55aa2bcf355441585bcf980e1d6f4fab8eebe59d`。三者分别记录，原 C 长时研究继续使用原提交及原预算。

## 可复核文件

- [round29-closeout.json](round29-closeout.json)：计数、源码范围、原验收结果和剩余条件。
- [round29-public-evidence.tar.gz](round29-public-evidence.tar.gz)：107 个归档路径，来自 106 个不同原文件，总 payload 1,554,607 bytes。相同的 C recovery helper 同行审查在 platform 与 runtime 两个范围各保留一份引用。
- [round29-public-evidence-inventory.json](round29-public-evidence-inventory.json)：逐文件路径、完整 SHA-256、Git blob 和 mode。
- [round29-public-evidence-readback.json](round29-public-evidence-readback.json)：全部 107 个成员完整解压读回与源文件未变化核验。
- [round29-public-bundle-inputs.json](round29-public-bundle-inputs.json) 与 [build_public_round29_bundle.py](build_public_round29_bundle.py)：固定输入和确定性容器构建代码。
- `details/`：本轮主要原件验收、原任务定义保留以及未来组合只读设计，便于直接审查。

容器为 284,259 bytes，SHA-256 `562f7855add368c77d4cf5c50813e7890bc183633316070680ca02ada20b47ae`，Git blob `a32879e3b42635bde5e2459f159cd3c0d1c85f24`。完整原始 GitHub ZIP 已保留；此公开容器收录验收证据、原完整日志及辅助记录，不宣称包含全部原 ZIP。私有传输凭据和捕获文件不在容器中。

恢复验收后的派生清理曾因一个旧产品二进制的文件 mode 与 preflight 记录不同而停止。该文件的字节与 ZIP 一致，原因未知；首次失败记录、该文件及父目录均被保留。随后仅对其余 498 个逐件重新核验的可重建派生文件执行清理。生命周期目录中来源未明的派生路径保持原样。

## 原任务定义与后续工作

当前任务文件仅有 12 处 evidence 后缀追加及 P8-017 / P8-018 的 implementation_notes 后缀追加。原定义、原前缀、依赖、acceptance_subgates 和状态保持一致。当前计数仍为 163 done、1 blocked、16 in_progress、12 todo。

下一轮继续接收原 C 的混合负载、soak、剩余平台与规模产物，等待 LG 的真实 CI 结果。未来 M2 + LP 的 Cargo 组合目前只有只读设计；任何实际源码组合都需要独立的新来源审查及源码对应的实际回归结果。

## 官方来源

- [PR #180](https://github.com/jyqj/codecortex/pull/180)
- [原 C 规模研究](https://github.com/jyqj/codecortex/actions/runs/37910924354)
- [原 C P7 closeout](https://github.com/jyqj/codecortex/actions/runs/37908825794)
- [原 C 平台与恢复](https://github.com/jyqj/codecortex/actions/runs/37908825722)
- [原 C runtime](https://github.com/jyqj/codecortex/actions/runs/37908825814)
- [LG 锁观测诊断](https://github.com/jyqj/codecortex/actions/runs/37919399759)
- [上一轮固定证据提交](https://github.com/jyqj/codecortex/commit/1f7e6816f1656b330cafc88f1a34a40d0d4c6086)

